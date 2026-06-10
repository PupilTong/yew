// Inspired by: http://package.elm-lang.org/packages/elm-lang/html/2.0.0/Html-Events
//
// The event payload is unit (`()`) for every listener kind; closures that need
// to look at modifier keys, coordinates, or the target element should call
// `lynx_sys::event_*` helpers during dispatch.

macro_rules! impl_action {
    ($($action:ident($passive:literal))*) => {$(
        /// An abstract implementation of a listener.
        #[doc(hidden)]
        pub mod $action {
            use crate::callback::Callback;
            use crate::virtual_dom::{Listener, ListenerKind};
            use std::rc::Rc;

            /// A wrapper for a callback which attaches event listeners to elements.
            #[derive(Clone, Debug)]
            pub struct Wrapper {
                callback: Callback<Event>,
            }

            impl Wrapper {
                /// Create a wrapper for an event-typed callback
                pub fn new(callback: Callback<Event>) -> Self {
                    Wrapper { callback }
                }

                #[doc(hidden)]
                #[inline]
                pub fn __macro_new(
                    callback: impl crate::html::IntoEventCallback<Event>,
                ) -> Option<Rc<dyn Listener>> {
                    let callback = callback.into_event_callback()?;
                    Some(Rc::new(Self::new(callback)))
                }
            }

            /// The event payload. Unit in the WAMR runtime; event details are
            /// queried from the host via `lynx_sys::event_*`.
            pub type Event = ();

            impl Listener for Wrapper {
                fn kind(&self) -> ListenerKind {
                    ListenerKind::$action
                }

                fn handle(&self, event: ()) {
                    self.callback.emit(event);
                }

                fn passive(&self) -> bool {
                    $passive
                }
            }
        }
    )*};
}

macro_rules! impl_active {
    ($($action:ident)*) => {
        impl_action! { $( $action(false) )* }
    };
}

macro_rules! impl_passive {
    ($($action:ident)*) => {
        impl_action! { $( $action(true) )* }
    };
}

// All non-passive listener kinds. The WAMR runtime no longer distinguishes the
// event payload type per kind; the only difference between wrappers is the
// `ListenerKind` enum tag and whether the listener is registered passive.
impl_active! {
    onabort
    onauxclick
    onblur
    oncancel
    oncanplay
    oncanplaythrough
    onchange
    onclick
    onclose
    oncontextmenu
    oncopy
    oncut
    oncuechange
    ondblclick
    ondrag
    ondragend
    ondragenter
    ondragexit
    ondragleave
    ondragover
    ondragstart
    ondrop
    ondurationchange
    onemptied
    onended
    onerror
    onfocus
    onfocusin
    onfocusout
    onformdata
    oninput
    oninvalid
    onkeydown
    onkeypress
    onkeyup
    onload
    onloadeddata
    onloadedmetadata
    onloadend
    onloadstart
    onmousedown
    onmouseenter
    onmouseleave
    onmousemove
    onmouseout
    onmouseover
    onmouseup
    onpaste
    onpause
    onplay
    onplaying
    onpointerlockchange
    onpointerlockerror
    onprogress
    onratechange
    onreset
    onresize
    onsecuritypolicyviolation
    onseeked
    onseeking
    onselect
    onselectionchange
    onselectstart
    onshow
    onslotchange
    onstalled
    onsubmit
    onsuspend
    ontap
    ontimeupdate
    ontoggle
    onvolumechange
    onwaiting
    onwheel

    onanimationcancel
    onanimationend
    onanimationiteration
    onanimationstart

    ongotpointercapture
    onlostpointercapture
    onpointercancel
    onpointerdown
    onpointerenter
    onpointerleave
    onpointermove
    onpointerout
    onpointerover
    onpointerup

    ontouchcancel
    ontouchend

    ontransitioncancel
    ontransitionend
    ontransitionrun
    ontransitionstart
}

// Best used with passive listeners for responsiveness
impl_passive! {
    onscroll

    ontouchmove
    ontouchstart
}
