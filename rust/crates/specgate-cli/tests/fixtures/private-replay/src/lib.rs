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
mod producer_scopes {
    use specgate::{SpecEvent, spec_operation, spec_setup};

    #[derive(SpecEvent)]
    struct ProducerClient {}

    #[derive(SpecEvent)]
    struct ProducerOffset {
        #[spec_event]
        value: i32,
    }

    impl ProducerClient {
        #[spec_operation("read", spec = "fixture.producer_scopes")]
        pub fn read(&self, offset: ProducerOffset) -> i32 {
            offset.value
        }
    }

    mod receiver {
        use super::*;

        #[spec_setup("read", spec = "fixture.producer_scopes")]
        pub(super) fn make() -> ProducerClient {
            ProducerClient {}
        }
    }

    mod parameter {
        use super::*;

        #[spec_setup("read", spec = "fixture.producer_scopes")]
        pub(super) fn make(value: i32) -> ProducerOffset {
            ProducerOffset { value }
        }
    }

    #[spec_setup("text_len", spec = "fixture.producer_scopes", fills = "value")]
    fn owned_text(value: String) -> String {
        value
    }

    #[spec_operation("text_len", spec = "fixture.producer_scopes")]
    pub fn text_len(value: &str) -> i32 {
        value.len() as i32
    }

    #[spec_setup("unit_input", spec = "fixture.producer_scopes", fills = "value")]
    fn owned_unit(seed: ()) -> () {
        seed
    }

    #[spec_operation("unit_input", spec = "fixture.producer_scopes")]
    pub fn unit_input(value: ()) -> i32 {
        let () = value;
        1
    }

    #[test]
    fn distinct_modules_and_owned_coercion() {
        assert_eq!(receiver::make().read(parameter::make(7)), 7);
        let value = owned_text("owned native storage".to_string());
        assert_eq!(text_len(&value), 20);
        assert_eq!(unit_input(owned_unit(())), 1);
    }
}

#[cfg(test)]
mod consuming {
    use specgate::{SpecEvent, spec_operation, spec_setup};

    #[derive(SpecEvent)]
    struct ConsumingClient {
        #[spec_event]
        value: String,
    }

    impl ConsumingClient {
        #[spec_operation("finish", spec = "fixture.consuming")]
        pub fn finish(self) -> String {
            self.value
        }
    }

    #[spec_setup("finish", spec = "fixture.consuming")]
    fn make(value: String) -> ConsumingClient {
        ConsumingClient { value }
    }

    #[test]
    fn capture_non_copy_consuming_receiver() {
        assert_eq!(make("consumed".to_string()).finish(), "consumed");
    }
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
