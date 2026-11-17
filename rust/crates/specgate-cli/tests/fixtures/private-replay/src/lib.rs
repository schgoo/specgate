use specgate::{SpecEvent, ToNativeValue, spec_operation};

specgate::spec_component!("fixture.private_replay");

pub struct Context(u8);

impl Default for Context {
    fn default() -> Self {
        Self(17)
    }
}

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
pub fn outer(#[spec_input(ignore)] context: Context) -> bool {
    assert_eq!(context.0, 17);
    inner().value == 1
}

#[cfg(test)]
mod producer_scopes {
    use super::Context;
    use specgate::{SpecEvent, spec_operation, spec_setup};

    #[derive(SpecEvent)]
    #[spec_event(setup)]
    #[spec_component("fixture.producer_scopes")]
    struct Seed {
        #[spec_event]
        pub value: i32,
    }

    #[derive(SpecEvent)]
    struct ProducerClient {}

    #[derive(SpecEvent)]
    struct ProducerOffset {
        #[spec_event]
        value: i32,
    }

    impl ProducerClient {
        #[spec_operation("read", component = "fixture.producer_scopes")]
        pub fn read(&self, #[spec_input(ignore)] context: Context, offset: ProducerOffset) -> i32 {
            assert_eq!(context.0, 17);
            offset.value
        }
    }

    mod receiver {
        use super::*;

        #[spec_setup("read", component = "fixture.producer_scopes")]
        pub(super) fn make() -> ProducerClient {
            ProducerClient {}
        }
    }

    mod parameter {
        use super::*;

        #[spec_setup("read", component = "fixture.producer_scopes")]
        pub(super) fn make(#[spec_input("value", ignore)] context: Context, value: Seed) -> ProducerOffset {
            assert_eq!(context.0, 17);
            ProducerOffset { value: value.value }
        }
    }

    #[spec_setup("text_len", component = "fixture.producer_scopes", fills = "value")]
    fn owned_text(#[spec_input(ignore)] values: Context, value: String) -> String {
        assert_eq!(values.0, 17);
        value
    }

    #[spec_operation("text_len", component = "fixture.producer_scopes")]
    pub fn text_len(value: &str) -> i32 {
        value.len() as i32
    }

    #[spec_setup("unit_input", component = "fixture.producer_scopes", fills = "value")]
    fn owned_unit(seed: ()) -> () {
        seed
    }

    #[spec_operation("unit_input", component = "fixture.producer_scopes")]
    pub fn unit_input(value: ()) -> i32 {
        let () = value;
        1
    }

    #[test]
    fn distinct_modules_and_owned_coercion() {
        assert_eq!(
            receiver::make().read(Context(17), parameter::make(Context(17), Seed { value: 7 })),
            7
        );
        let value = owned_text(Context(17), "owned native storage".to_string());
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
        #[spec_operation("finish", component = "fixture.consuming")]
        pub fn finish(self) -> String {
            self.value
        }
    }

    #[spec_setup("finish", component = "fixture.consuming")]
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
        use super::{Client, Context, Offset};
        use crate::Item;
        use specgate::{ToNativeValue, spec_operation};

        impl Client {
            #[spec_operation("read", types = "Item")]
            pub fn read<'a, T: From<i32> + ToNativeValue + 'static>(
                &mut self,
                #[spec_input("offset", ignore)] context: Context,
                offset: &'a mut Offset,
            ) -> Vec<T> {
                assert_eq!(context.0, 17);
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
        assert_eq!(client.read::<crate::Item>(Context(17), &mut offset)[0].value, 7);
        assert_eq!(client.read::<crate::Item>(Context(17), &mut offset)[0].value, 7);
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
        assert!(outer(Context(17)));
    }

    #[spec_operation("plain", component = "fixture.test_only")]
    pub fn plain(value: i32) -> i32 {
        value + 1
    }

    #[test]
    fn test_only_operation() {
        assert_eq!(plain(4), 5);
    }
}
