use std::{
    fmt::Debug,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use tokio::sync::mpsc::{Receiver, Sender, error::TryRecvError};
use tracing::{debug, error};

/// `EventBus<T>` is the data structure behind the implementation of
/// event bus functionality, event bus allows to create channel and channel
/// handlers to exchange event/messages of type T (usually used of enum events)
pub struct EventBus<T> {
    /// `channel_handlers` represents the list of registered handlers
    channel_handlers: Arc<Mutex<Vec<ChannelHandler<T>>>>,
}

type ChannelHandlerCallback<T> = Box<dyn Fn(T) -> Result<()> + Send + Sync>;

/// `ChannelHandler<T>` is a inner data struct representing the registered
/// channel receiver and channel handler
struct ChannelHandler<T> {
    /// `receiver` is used to listen for channel events/messages
    receiver: Receiver<T>,
    /// `handler` is registered process/function to react and do something
    /// on received event/message
    handler: ChannelHandlerCallback<T>,
}

impl<T: Debug + Send + 'static> Default for EventBus<T> {
    fn default() -> Self {
        Self {
            channel_handlers: Arc::new(Mutex::new(vec![])),
        }
    }
}

impl<T: Debug + Send + 'static> EventBus<T> {
    /// `EventBus::new()` creates a new instance of `EventBus` service
    pub fn new() -> Self {
        Self {
            channel_handlers: Arc::new(Mutex::new(vec![])),
        }
    }

    /// method responsible for creating a new channel as a part of the `EventBus`
    /// inner state and providing caller the `Sender<T>` part of the channel together
    /// with function responsible registering the channel handler
    pub fn create_channel(
        &self,
    ) -> (
        Sender<T>,
        impl FnOnce(ChannelHandlerCallback<T>) + Send + 'static,
    ) {
        let (tx, rx) = tokio::sync::mpsc::channel::<T>(8);

        let handlers = Arc::clone(&self.channel_handlers);

        let register = move |handler: Box<dyn Fn(T) -> Result<()> + Send + Sync>| {
            let channel_handler = ChannelHandler {
                receiver: rx,
                handler,
            };
            handlers.lock().unwrap().push(channel_handler);
        };

        (tx, register)
    }

    /// method responsible for checking for new events/messages from created channels
    /// and calling the registered channel handlers
    pub fn try_process(&self) {
        let mut channel_handlers = self.channel_handlers.lock().unwrap();
        channel_handlers.retain_mut(|channel_handler| {
            loop {
                match channel_handler.receiver.try_recv() {
                    Ok(event) => {
                        debug!("Event from channel receiver: {:?}", event);
                        if let Err(error) = (channel_handler.handler)(event) {
                            error!(
                                "Error occurred while handling the event, error: {:?}",
                                error
                            );
                        }
                    }
                    Err(TryRecvError::Empty) => return true,
                    Err(TryRecvError::Disconnected) => return false,
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use anyhow::anyhow;

    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[derive(Debug, PartialEq, Eq, Clone)]
    enum TestEvent {
        Foo(u32),
        Bar(String),
    }

    #[test]
    fn test_event_bus_single_handler() {
        let bus = EventBus::<TestEvent>::new();
        let (tx, register) = bus.create_channel();

        // Use Arc<Mutex<...>> to share state between handler and test
        let called = Arc::new(Mutex::new(false));
        let called_clone = Arc::clone(&called);

        register(Box::new(move |event| {
            if let TestEvent::Foo(val) = event {
                *called_clone.lock().unwrap() = true;
                assert_eq!(val, 42);
            }
            Ok(())
        }));

        // Send an event
        tx.blocking_send(TestEvent::Foo(42)).unwrap();

        // Process events
        bus.try_process();

        // Check that handler was called
        assert_eq!(*called.lock().unwrap(), true);
    }

    #[test]
    fn test_event_bus_multiple_handlers() {
        let bus = EventBus::<TestEvent>::new();
        let (tx1, register1) = bus.create_channel();
        let (tx2, register2) = bus.create_channel();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter1 = Arc::clone(&counter);
        let counter2 = Arc::clone(&counter);

        register1(Box::new(move |event| {
            if let TestEvent::Bar(ref s) = event {
                if s == "hello" {
                    counter1.fetch_add(1, Ordering::SeqCst);
                }
            }
            Ok(())
        }));

        register2(Box::new(move |event| {
            if let TestEvent::Bar(ref s) = event {
                if s == "hello" {
                    counter2.fetch_add(1, Ordering::SeqCst);
                }
            }
            Ok(())
        }));

        tx1.blocking_send(TestEvent::Bar("hello".to_string()))
            .unwrap();
        tx2.blocking_send(TestEvent::Bar("hello".to_string()))
            .unwrap();

        bus.try_process();
        bus.try_process();

        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_event_bus_handler_error() {
        let bus = EventBus::<TestEvent>::new();
        let (tx, register) = bus.create_channel();

        register(Box::new(move |_event| Err(anyhow!("handler error"))));

        tx.blocking_send(TestEvent::Foo(1)).unwrap();

        // Should not panic, error is just logged
        bus.try_process();
    }

    #[test]
    fn test_event_bus_no_handler_registered() {
        let bus = EventBus::<TestEvent>::new();
        let (tx, _register) = bus.create_channel();
        tx.blocking_send(TestEvent::Foo(99)).unwrap();
        // No handler registered, should not panic
        bus.try_process();
    }

    #[test]
    fn test_event_bus_closed_channel_is_cleaned_up() {
        let bus = EventBus::<TestEvent>::new();
        let (tx, register) = bus.create_channel();

        let called = Arc::new(Mutex::new(false));
        let called_clone = Arc::clone(&called);
        register(Box::new(move |_event| {
            *called_clone.lock().unwrap() = true;
            Ok(())
        }));

        drop(tx);
        bus.try_process();

        assert_eq!(bus.channel_handlers.lock().unwrap().len(), 0);
        assert_eq!(*called.lock().unwrap(), false);
    }

    #[test]
    fn test_event_bus_closed_channel_with_pending_messages_is_cleaned_up() {
        let bus = EventBus::<TestEvent>::new();
        let (tx, register) = bus.create_channel();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);
        register(Box::new(move |event| {
            if let TestEvent::Foo(val) = event {
                counter_clone.fetch_add(val as usize, Ordering::SeqCst);
            }
            Ok(())
        }));

        tx.blocking_send(TestEvent::Foo(5)).unwrap();
        drop(tx);

        bus.try_process();

        assert_eq!(counter.load(Ordering::SeqCst), 5);
        assert_eq!(bus.channel_handlers.lock().unwrap().len(), 0);
    }

    #[test]
    fn test_event_bus_multiple_events() {
        let bus = EventBus::<TestEvent>::new();
        let (tx, register) = bus.create_channel();

        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        register(Box::new(move |_event| {
            counter_clone.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }));

        tx.blocking_send(TestEvent::Foo(1)).unwrap();
        tx.blocking_send(TestEvent::Foo(2)).unwrap();

        bus.try_process();
        bus.try_process();

        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}
