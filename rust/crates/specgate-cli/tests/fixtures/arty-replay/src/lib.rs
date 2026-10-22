specgate::spec_component!("fixture.arty_replay");

#[cfg(test)]
pub(crate) mod tests {
    use candidate_engine::{runtime::Runtime, task::Builtins};
    use specgate::{SpecEvent, spec_operation, spec_setup};
    use std::{
        cell::{Cell, RefCell},
        future::Future,
        rc::Rc,
    };

    thread_local! {
        static BUILTINS: RefCell<Option<Builtins>> = const { RefCell::new(None) };
        static SETUP_ENTERED: Cell<bool> = const { Cell::new(false) };
    }

    struct Context;

    impl Drop for Context {
        fn drop(&mut self) {
            BUILTINS.set(None);
        }
    }

    pub(crate) fn replay_executor<F, Fut>(task: F) -> Result<(), String>
    where
        F: FnOnce() -> Fut + Send,
        Fut: Future<Output = Result<(), String>>,
    {
        let runtime = Runtime::new().map_err(|error| error.to_string())?;
        let result = runtime.scheduler().block_on(async move |cx| {
            BUILTINS.set(Some(cx));
            SETUP_ENTERED.set(false);
            let _context = Context;
            let result = task().await;
            assert!(SETUP_ENTERED.get(), "custom executor must enter before setup construction");
            result
        });
        runtime.stop().map_err(|error| error.to_string())?;
        result.map_err(|error| error.to_string())?
    }

    #[derive(SpecEvent)]
    pub struct Client {
        value: Rc<u32>,
        clock: Option<candidate_engine::time::Clock>,
    }

    #[derive(SpecEvent)]
    pub struct Item {
        #[spec_event]
        pub value: u32,
    }

    #[spec_setup("read")]
    fn make_client(seed: u32) -> Client {
        let clock = BUILTINS.with(|value| value.borrow().as_ref().map(|cx| cx.clock().clone()));
        if clock.is_some() {
            SETUP_ENTERED.set(true);
        }
        Client {
            value: Rc::new(seed),
            clock,
        }
    }

    impl Client {
        #[spec_operation("read")]
        pub async fn read(&self) -> Vec<Item> {
            if let Some(clock) = &self.clock {
                clock.delay(std::time::Duration::from_millis(1)).await;
            } else {
                let mut yielded = false;
                std::future::poll_fn(|cx| {
                    if yielded {
                        std::task::Poll::Ready(())
                    } else {
                        yielded = true;
                        cx.waker().wake_by_ref();
                        std::task::Poll::Pending
                    }
                })
                .await;
            }
            vec![Item { value: *self.value }]
        }
    }

    #[test]
    fn captures_with_the_configured_runtime() {
        replay_executor(|| async {
            let client = make_client(u32::MAX);
            assert_eq!(client.read().await[0].value, u32::MAX);
            Ok(())
        })
        .expect("candidate runtime");
    }
}
