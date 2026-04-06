use crate::html::{Html, Properties};

/// Properties for [Suspense].
#[derive(Properties, PartialEq, Debug, Clone)]
pub struct SuspenseProps {
    /// The Children of the current Suspense Component.
    #[prop_or_default]
    pub children: Html,

    /// The Fallback UI of the current Suspense Component.
    #[prop_or_default]
    pub fallback: Html,
}

#[cfg(feature = "csr")]
mod feat_csr_ssr {
    use super::*;
    use crate::html::{Component, Context, Html, Scope};
    use crate::suspense::Suspension;
    use crate::virtual_dom::{VNode, VSuspense};
    use crate::{component, html};

    #[derive(Properties, PartialEq, Debug, Clone)]
    pub(crate) struct BaseSuspenseProps {
        pub children: Html,
        #[prop_or(None)]
        pub fallback: Option<Html>,
    }

    #[derive(Debug)]
    pub(crate) enum BaseSuspenseMsg {
        Suspend(Suspension),
        Resume(Suspension),
    }

    #[derive(Debug)]
    pub(crate) struct BaseSuspense {
        suspensions: Vec<Suspension>,
    }

    impl Component for BaseSuspense {
        type Message = BaseSuspenseMsg;
        type Properties = BaseSuspenseProps;

        fn create(_ctx: &Context<Self>) -> Self {
            Self {
                suspensions: Vec::new(),
            }
        }

        fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
            match msg {
                Self::Message::Suspend(m) => {
                    assert!(
                        ctx.props().fallback.is_some(),
                        "You cannot suspend from a component rendered as a fallback."
                    );

                    if m.resumed() {
                        return false;
                    }

                    // If a suspension already exists, ignore it.
                    if self.suspensions.iter().any(|n| n == &m) {
                        return false;
                    }

                    self.suspensions.push(m);

                    true
                }
                Self::Message::Resume(ref m) => {
                    let suspensions_len = self.suspensions.len();
                    self.suspensions.retain(|n| m != n);

                    suspensions_len != self.suspensions.len()
                }
            }
        }

        fn view(&self, ctx: &Context<Self>) -> Html {
            let BaseSuspenseProps { children, fallback } = (*ctx.props()).clone();
            let children = html! {<>{children}</>};

            match fallback {
                Some(fallback) => {
                    let vsuspense = VSuspense::new(
                        children,
                        fallback,
                        !self.suspensions.is_empty(),
                        // We don't need to key this as the key will be applied to the component.
                        None,
                    );

                    VNode::from(vsuspense)
                }
                None => children,
            }
        }

    }

    impl BaseSuspense {
        pub(crate) fn suspend(scope: &Scope<Self>, s: Suspension) {
            scope.send_message(BaseSuspenseMsg::Suspend(s));
        }

        pub(crate) fn resume(scope: &Scope<Self>, s: Suspension) {
            scope.send_message(BaseSuspenseMsg::Resume(s));
        }
    }

    /// Suspend rendering and show a fallback UI until the underlying task completes.
    #[component]
    pub fn Suspense(props: &SuspenseProps) -> Html {
        let SuspenseProps { children, fallback } = props.clone();

        let fallback = html! {
            <BaseSuspense>
                {fallback}
            </BaseSuspense>
        };

        html! {
            <BaseSuspense {fallback}>
                {children}
            </BaseSuspense>
        }
    }
}

#[cfg(feature = "csr")]
pub use feat_csr_ssr::*;

#[cfg(not(feature = "csr"))]
mod feat_no_csr_ssr {
    use super::*;
    use crate::component;

    /// Suspend rendering and show a fallback UI until the underlying task completes.
    #[component]
    pub fn Suspense(_props: &SuspenseProps) -> Html {
        Html::default()
    }
}

#[cfg(not(feature = "csr"))]
pub use feat_no_csr_ssr::*;
