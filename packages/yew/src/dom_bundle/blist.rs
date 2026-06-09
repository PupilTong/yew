//! This module contains fragments bundles, a [BList]
use std::borrow::Borrow;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::hash::Hash;
use std::ops::Deref;
use std::rc::Rc;

use lynx_sys::Element;

use super::{test_log, BNode, BSubtree, DomSlot};
use crate::dom_bundle::{Reconcilable, ReconcileTarget};
use crate::html::AnyScope;
use crate::utils::RcExt;
use crate::virtual_dom::{Key, VList, VNode};

/// This struct represents a mounted [VList]
#[derive(Debug)]
pub(super) struct BList {
    /// The reverse (render order) list of child [BNode]s
    rev_children: Vec<BNode>,
    /// All [BNode]s in the BList have keys
    fully_keyed: bool,
    key: Option<Key>,
}

impl VList {
    // Splits a VList for creating / reconciling to a BList.
    fn split_for_blist(self) -> (Option<Key>, bool, Vec<VNode>) {
        let fully_keyed = self.fully_keyed();

        let children = self
            .children
            .map(RcExt::unwrap_or_clone)
            .unwrap_or_default();

        (self.key, fully_keyed, children)
    }
}

impl Deref for BList {
    type Target = Vec<BNode>;

    fn deref(&self) -> &Self::Target {
        &self.rev_children
    }
}

/// Helper struct, that keeps the position where the next element is to be placed at
#[derive(Clone)]
struct NodeWriter<'s> {
    root: &'s BSubtree,
    parent_scope: &'s AnyScope,
    parent: &'s Rc<Element>,
    slot: DomSlot,
}

impl NodeWriter<'_> {
    /// Write a new node that has no ancestor
    fn add(self, node: VNode) -> (Self, BNode) {
        test_log!("adding: {:?}", node);
        test_log!("  parent={:?}, slot={:?}", self.parent, self.slot);
        let (next, bundle) = node.attach(self.root, self.parent_scope, self.parent, self.slot);
        test_log!("  next_slot: {:?}", next);
        (Self { slot: next, ..self }, bundle)
    }

    /// Shift a bundle into place without patching it
    fn shift(&self, bundle: &BNode) {
        bundle.shift(self.parent, self.slot.clone());
    }

    /// Patch a bundle with a new node
    fn patch(self, node: VNode, bundle: &mut BNode) -> Self {
        test_log!("patching: {:?} -> {:?}", bundle, node);
        test_log!("  parent={:?}, slot={:?}", self.parent, self.slot);
        // Advance the next sibling reference (from right to left)
        let next =
            node.reconcile_node(self.root, self.parent_scope, self.parent, self.slot, bundle);
        test_log!("  next_position: {:?}", next);
        Self { slot: next, ..self }
    }
}
/// Helper struct implementing [Eq] and [Hash] by only looking at a node's key
struct KeyedEntry(usize, BNode);
impl Borrow<Key> for KeyedEntry {
    fn borrow(&self) -> &Key {
        self.1.key().expect("unkeyed child in fully keyed list")
    }
}
impl Hash for KeyedEntry {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        <Self as Borrow<Key>>::borrow(self).hash(state)
    }
}
impl PartialEq for KeyedEntry {
    fn eq(&self, other: &Self) -> bool {
        <Self as Borrow<Key>>::borrow(self) == <Self as Borrow<Key>>::borrow(other)
    }
}
impl Eq for KeyedEntry {}

impl BNode {
    /// Assert that a bundle node is a list, or convert it to a list with a single child
    fn make_list(&mut self) -> &mut BList {
        match self {
            Self::List(blist) => blist,
            self_ => {
                let b = std::mem::replace(self_, BNode::List(BList::new()));
                let self_list = match self_ {
                    BNode::List(blist) => blist,
                    _ => unreachable!("just been set to the variant"),
                };
                let key = b.key().cloned();
                self_list.rev_children.push(b);
                self_list.fully_keyed = key.is_some();
                self_list.key = key;
                self_list
            }
        }
    }
}

impl BList {
    /// Create a new empty [BList]
    pub const fn new() -> BList {
        BList {
            rev_children: vec![],
            fully_keyed: true,
            key: None,
        }
    }

    /// Get the key of the underlying fragment
    pub fn key(&self) -> Option<&Key> {
        self.key.as_ref()
    }

    /// Diff and patch unkeyed child lists
    fn apply_unkeyed(
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        slot: DomSlot,
        lefts: Vec<VNode>,
        rights: &mut Vec<BNode>,
    ) -> DomSlot {
        let mut writer = NodeWriter {
            root,
            parent_scope,
            parent,
            slot,
        };

        // Remove extra nodes
        if lefts.len() < rights.len() {
            for r in rights.drain(lefts.len()..) {
                test_log!("removing: {:?}", r);
                r.detach(root, parent, false);
            }
        }

        let mut lefts_it = lefts.into_iter().rev();
        for (r, l) in rights.iter_mut().zip(&mut lefts_it) {
            writer = writer.patch(l, r);
        }

        // Add missing nodes
        for l in lefts_it {
            let (next_writer, el) = writer.add(l);
            rights.push(el);
            writer = next_writer;
        }
        writer.slot
    }

    /// Diff and patch fully keyed child lists.
    ///
    /// Optimized for node addition or removal from either end of the list and small changes in the
    /// middle.
    fn apply_keyed(
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        slot: DomSlot,
        left_vdoms: Vec<VNode>,
        rev_bundles: &mut Vec<BNode>,
    ) -> DomSlot {
        macro_rules! key {
            ($v:expr) => {
                $v.key().expect("unkeyed child in fully keyed list")
            };
        }
        /// Find the first differing key in 2 iterators
        fn matching_len<'a, 'b>(
            a: impl Iterator<Item = &'a Key>,
            b: impl Iterator<Item = &'b Key>,
        ) -> usize {
            a.zip(b).take_while(|(a, b)| a == b).count()
        }

        // Find first key mismatch from the back
        let matching_len_end = matching_len(
            left_vdoms.iter().map(|v| key!(v)).rev(),
            rev_bundles.iter().map(|v| key!(v)),
        );

        if cfg!(debug_assertions) {
            let mut keys = HashSet::with_capacity(left_vdoms.len());
            for (idx, n) in left_vdoms.iter().enumerate() {
                let key = key!(n);
                debug_assert!(
                    keys.insert(key!(n)),
                    "duplicate key detected: {key} at index {idx}. Keys in keyed lists must be \
                     unique!",
                );
            }
        }

        // If there is no key mismatch, apply the unkeyed approach
        // Corresponds to adding or removing items from the back of the list
        if matching_len_end == std::cmp::min(left_vdoms.len(), rev_bundles.len()) {
            // No key changes
            return Self::apply_unkeyed(root, parent_scope, parent, slot, left_vdoms, rev_bundles);
        }

        // We partially drain the new vnodes in several steps.
        let mut lefts = left_vdoms;
        let mut writer = NodeWriter {
            root,
            parent_scope,
            parent,
            slot,
        };
        // Step 1. Diff matching children at the end
        let lefts_to = lefts.len() - matching_len_end;
        for (l, r) in lefts
            .drain(lefts_to..)
            .rev()
            .zip(rev_bundles[..matching_len_end].iter_mut())
        {
            writer = writer.patch(l, r);
        }

        // Step 2. Diff matching children in the middle, that is between the first and last key
        // mismatch Find first key mismatch from the front
        let mut matching_len_start = matching_len(
            lefts.iter().map(|v| key!(v)),
            rev_bundles.iter().map(|v| key!(v)).rev(),
        );

        // Step 2.1. Splice out the existing middle part and build a lookup by key
        let rights_to = rev_bundles.len() - matching_len_start;
        let mut bundle_middle = matching_len_end..rights_to;
        if bundle_middle.start > bundle_middle.end {
            // If this range is "inverted", this implies that the incoming nodes in lefts contain a
            // duplicate key!
            // Pictogram:
            //                                         v lefts_to
            // lefts:              | SSSSSSSS | ------ | EEEEEEEE |
            //                                ↕ matching_len_start
            // rev_bundles.rev():  | SSS | ?? | EEE |
            //                           ^ rights_to
            // Both a key from the (S)tarting portion and (E)nding portion of lefts has matched a
            // key in the ? portion of bundles. Since the former can't overlap, a key
            // must be duplicate. Duplicates might lead to us forgetting about some
            // bundles entirely. It is NOT straight forward to adjust the below code to
            // consistently check and handle this. The duplicate keys might
            // be in the start or end portion.
            // With debug_assertions we can never reach this. For production code, hope for the best
            // by pretending. We still need to adjust some things so splicing doesn't
            // panic:
            matching_len_start = 0;
            bundle_middle = matching_len_end..rev_bundles.len();
        }
        let (matching_len_start, bundle_middle) = (matching_len_start, bundle_middle);

        // BNode contains js objects that look suspicious to clippy but are harmless
        #[allow(clippy::mutable_key_type)]
        let mut spare_bundles: HashSet<KeyedEntry> = HashSet::with_capacity(bundle_middle.len());
        let mut spliced_middle = rev_bundles.splice(bundle_middle, std::iter::empty());
        for (idx, r) in (&mut spliced_middle).enumerate() {
            #[cold]
            fn duplicate_in_bundle(root: &BSubtree, parent: &Rc<Element>, r: BNode) {
                test_log!("removing: {:?}", r);
                r.detach(root, parent, false);
            }
            if let Some(KeyedEntry(_, dup)) = spare_bundles.replace(KeyedEntry(idx, r)) {
                duplicate_in_bundle(root, parent, dup);
            }
        }

        // Step 2.2. Put the middle part back together in the new key order
        let mut replacements: Vec<BNode> = Vec::with_capacity((matching_len_start..lefts_to).len());
        // The goal is to shift as few nodes as possible.

        // We handle runs of in-order nodes. When we encounter one out-of-order, we decide whether:
        // - to shift all nodes in the current run to the position after the node before of the run,
        //   or to
        // - "commit" to the current run, shift all nodes before the end of the run that we might
        //   encounter in the future, and then start a new run.
        // Example of a run:
        //               barrier_idx --v                   v-- end_idx
        // spliced_middle  [ ... , M , N , C , D , E , F , G , ... ] (original element order)
        //                                 ^---^-----------^ the nodes that are part of the current
        // run                           v start_writer
        // replacements    [ ... , M , C , D , G ]                   (new element order)
        //                             ^-- start_idx
        let mut barrier_idx = 0; // nodes from spliced_middle[..barrier_idx] are shifted unconditionally
        struct RunInformation<'a> {
            start_writer: NodeWriter<'a>,
            start_idx: usize,
            end_idx: usize,
        }
        let mut current_run: Option<RunInformation<'_>> = None;

        for l in lefts
            .drain(matching_len_start..) // lefts_to.. has been drained
            .rev()
        {
            let ancestor = spare_bundles.take(key!(l));
            // Check if we need to shift or commit a run
            if let Some(run) = current_run.as_mut() {
                if let Some(KeyedEntry(idx, _)) = ancestor {
                    // If there are only few runs, this is a cold path
                    if idx < run.end_idx {
                        // Have to decide whether to shift or commit the current run. A few
                        // calculations: A perfect estimate of the amount of
                        // nodes we have to shift if we move this run:
                        let run_length = replacements.len() - run.start_idx;
                        // A very crude estimate of the amount of nodes we will have to shift if we
                        // commit the run: Note nodes of the current run
                        // should not be counted here!
                        let estimated_skipped_nodes = run.end_idx - idx.max(barrier_idx);
                        // double run_length to counteract that the run is part of the
                        // estimated_skipped_nodes
                        if 2 * run_length > estimated_skipped_nodes {
                            // less work to commit to this run
                            barrier_idx = 1 + run.end_idx;
                        } else {
                            // Less work to shift this run
                            for r in replacements[run.start_idx..].iter_mut().rev() {
                                run.start_writer.shift(r);
                            }
                        }
                        current_run = None;
                    }
                }
            }
            let bundle = if let Some(KeyedEntry(idx, mut r_bundle)) = ancestor {
                match current_run.as_mut() {
                    // hot path
                    // We know that idx >= run.end_idx, so this node doesn't need to shift
                    Some(run) => run.end_idx = idx,
                    None => match idx.cmp(&barrier_idx) {
                        // peep hole optimization, don't start a run as the element is already where
                        // it should be
                        Ordering::Equal => barrier_idx += 1,
                        // shift the node unconditionally, don't start a run
                        Ordering::Less => writer.shift(&r_bundle),
                        // start a run
                        Ordering::Greater => {
                            current_run = Some(RunInformation {
                                start_writer: writer.clone(),
                                start_idx: replacements.len(),
                                end_idx: idx,
                            })
                        }
                    },
                }
                writer = writer.patch(l, &mut r_bundle);
                r_bundle
            } else {
                // Even if there is an active run, we don't have to modify it
                let (next_writer, bundle) = writer.add(l);
                writer = next_writer;
                bundle
            };
            replacements.push(bundle);
        }
        // drop the splice iterator and immediately replace the range with the reordered elements
        drop(spliced_middle);
        rev_bundles.splice(matching_len_end..matching_len_end, replacements);

        // Step 2.3. Remove any extra rights
        for KeyedEntry(_, r) in spare_bundles.drain() {
            test_log!("removing: {:?}", r);
            r.detach(root, parent, false);
        }

        // Step 3. Diff matching children at the start
        let rights_to = rev_bundles.len() - matching_len_start;
        for (l, r) in lefts
            .drain(..) // matching_len_start.. has been drained already
            .rev()
            .zip(rev_bundles[rights_to..].iter_mut())
        {
            writer = writer.patch(l, r);
        }

        writer.slot
    }
}

impl ReconcileTarget for BList {
    fn detach(self, root: &BSubtree, parent: &Rc<Element>, parent_to_detach: bool) {
        for child in self.rev_children.into_iter() {
            child.detach(root, parent, parent_to_detach);
        }
    }

    fn shift(&self, next_parent: &Rc<Element>, mut slot: DomSlot) -> DomSlot {
        for node in self.rev_children.iter() {
            slot = node.shift(next_parent, slot);
        }

        slot
    }
}

impl Reconcilable for VList {
    type Bundle = BList;

    fn attach(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        slot: DomSlot,
    ) -> (DomSlot, Self::Bundle) {
        let mut self_ = BList::new();
        let node_ref = self.reconcile(root, parent_scope, parent, slot, &mut self_);
        (node_ref, self_)
    }

    fn reconcile_node(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        slot: DomSlot,
        bundle: &mut BNode,
    ) -> DomSlot {
        // 'Forcefully' pretend the existing node is a list. Creates a
        // singleton list if it isn't already.
        let blist = bundle.make_list();
        self.reconcile(root, parent_scope, parent, slot, blist)
    }

    fn reconcile(
        self,
        root: &BSubtree,
        parent_scope: &AnyScope,
        parent: &Rc<Element>,
        slot: DomSlot,
        blist: &mut BList,
    ) -> DomSlot {
        // Here, we will try to diff the previous list elements with the new
        // ones we want to insert. For that, we will use two lists:
        //  - lefts: new elements to render in the DOM
        //  - rights: previously rendered elements.
        //
        // The left items are known since we want to insert them
        // (self.children). For the right ones, we will look at the bundle,
        // i.e. the current DOM list element that we want to replace with self.
        let (key, fully_keyed, lefts) = self.split_for_blist();

        let rights = &mut blist.rev_children;
        test_log!("lefts: {:?}", lefts);
        test_log!("rights: {:?}", rights);

        if let Some(additional) = lefts.len().checked_sub(rights.len()) {
            rights.reserve_exact(additional);
        }
        let first = if fully_keyed && blist.fully_keyed {
            BList::apply_keyed(root, parent_scope, parent, slot, lefts, rights)
        } else {
            BList::apply_unkeyed(root, parent_scope, parent, slot, lefts, rights)
        };
        blist.fully_keyed = fully_keyed;
        blist.key = key;
        test_log!("result: {:?}", rights);
        first
    }
}
