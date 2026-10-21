use specgate::{SpecEvent, ToNativeValue, spec_operation};

specgate::spec_component!("fixture.private_replay");

mod model {
    use super::*;

    #[derive(SpecEvent)]
    pub struct Item {
        #[spec_event]
        pub value: i32,
    }

    impl From<i32> for Item {
        fn from(value: i32) -> Self {
            Self { value }
        }
    }

    #[spec_operation("free_items", types = "crate::Item")]
    pub fn free_items<T: From<i32> + ToNativeValue + 'static>(value: i32) -> Vec<T> {
        vec![T::from(value)]
    }
}

pub use model::{Item, free_items};

#[derive(SpecEvent)]
pub struct NestedItem {
    #[spec_event]
    pub value: i32,
}

#[spec_operation("inner")]
pub fn inner() -> NestedItem {
    NestedItem { value: 1 }
}

#[spec_operation("outer")]
pub fn outer() -> bool {
    inner().value == 1
}

#[cfg(test)]
mod tests {
    use super::*;
    // A different Item in the producer scope must not resolve method specializations.
    use crate::NestedItem as Item;
    use specgate::spec_setup;
    use std::sync::atomic::{AtomicBool, Ordering};

    static LEASED: AtomicBool = AtomicBool::new(false);

    #[derive(SpecEvent)]
    struct Client {
        calls: i32,
    }

    impl Drop for Client {
        fn drop(&mut self) {
            assert!(LEASED.swap(false, Ordering::SeqCst));
        }
    }

    #[derive(SpecEvent)]
    struct Offset {
        #[spec_event]
        value: i32,
    }

    mod operations {
        use super::{Client, Offset};
        use crate::Item;
        use specgate::{ToNativeValue, spec_operation};

        impl Client {
            #[spec_operation("read", types = "Item")]
            pub fn read<'a, T: From<i32> + ToNativeValue + 'static>(&mut self, offset: &'a mut Offset) -> Vec<T> {
                self.calls += 1;
                vec![T::from(offset.value)]
            }

            #[spec_operation("read_self", types = "self::Item")]
            pub fn read_self<T: From<i32> + ToNativeValue + 'static>(&mut self, offset: &mut Offset) -> Vec<T> {
                self.calls += 1;
                vec![T::from(offset.value)]
            }

            #[spec_operation("calls")]
            pub fn calls(&self) -> i32 {
                self.calls
            }
        }
    }

    #[spec_setup("read")]
    #[spec_setup("read_self")]
    #[spec_setup("calls")]
    fn make_client() -> Client {
        assert!(
            !LEASED.swap(true, Ordering::SeqCst),
            "previous scenario must release its exclusive environment"
        );
        Client { calls: 0 }
    }

    #[spec_operation("inferred")]
    pub fn inferred(offset: Offset) -> i32 {
        offset.value
    }

    #[spec_setup("read", fills = "offset")]
    #[spec_setup("read_self", fills = "offset")]
    #[spec_setup("inferred")]
    fn make_offset(seed: i32) -> Offset {
        Offset { value: seed }
    }

    fn scenario() {
        assert!(std::env::var_os("SPECGATE_TEST_CAPTURE").is_none());
        let mut client = make_client();
        let mut offset = make_offset(7);
        assert_eq!(client.read::<crate::Item>(&mut offset)[0].value, 7);
        assert_eq!(client.read::<crate::Item>(&mut offset)[0].value, 7);
        assert_eq!(client.read_self::<crate::Item>(&mut offset)[0].value, 7);
        assert_eq!(client.calls(), 3);
        assert_eq!(inferred(make_offset(7)), 7);
        let _: Option<Item> = None;
    }

    #[test]
    fn first_client() {
        scenario();
    }

    #[test]
    fn second_client() {
        scenario();
    }

    #[test]
    fn reexported_specialization() {
        assert!(std::env::var_os("SPECGATE_TEST_CAPTURE").is_none());
        assert_eq!(free_items::<crate::Item>(11)[0].value, 11);
    }

    #[test]
    fn nested_outcome() {
        assert!(outer());
    }

    #[spec_operation("plain", spec = "fixture.test_only")]
    pub fn plain(value: i32) -> i32 {
        value + 1
    }

    #[test]
    fn test_only_operation() {
        assert_eq!(plain(4), 5);
    }
}
