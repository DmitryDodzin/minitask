#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]
#![deny(unused_crate_dependencies)]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use async_channel::{Receiver, Sender};

mod future;
mod stream;
mod tasks;

pub use tasks::{BackgroundTasks, TaskSender, TaskUpdate};

pub trait BackgroundTask {
  type Task: Future;

  /// Incoming message type
  type MessageIn;

  /// Outgoing message type
  type MessageOut;

  /// Start the task with the provided [`MessageBus`]
  fn run(self, message_bus: MessageBus<Self::MessageIn, Self::MessageOut>) -> Self::Task;
}

/// Combined [`Receiver`] and [`Sender`] for inbound and outbound messages respectivly.
#[derive(Clone)]
pub struct MessageBus<MessageIn, MessageOut> {
  tx: Sender<MessageOut>,
  rx: Receiver<MessageIn>,
}

impl<MessageIn, MessageOut> MessageBus<MessageIn, MessageOut> {
  /// Create new bus from sender and reciver
  fn from_parts(tx: Sender<MessageOut>, rx: Receiver<MessageIn>) -> Self {
    MessageBus { tx, rx }
  }

  /// Split the bus to it's parts
  pub fn split(self) -> (Sender<MessageOut>, Receiver<MessageIn>) {
    let MessageBus { tx, rx } = self;
    (tx, rx)
  }

  pub fn send(&self, message: MessageOut) -> async_channel::Send<'_, MessageOut> {
    self.tx.send(message)
  }

  pub fn recv(&self) -> async_channel::Recv<'_, MessageIn> {
    self.rx.recv()
  }
}

#[cfg(test)]
mod tests {

  use alloc::{boxed::Box, format, string::String};
  use core::pin::Pin;

  use smol::stream::StreamExt;

  use super::*;

  struct TestTask;

  impl BackgroundTask for TestTask {
    type Task = smol::Task<Result<(), async_channel::SendError<String>>>;

    type MessageIn = u32;
    type MessageOut = String;

    fn run(self, message_bus: MessageBus<Self::MessageIn, Self::MessageOut>) -> Self::Task {
      smol::spawn(async move {
        while let Ok(message) = message_bus.recv().await {
          message_bus.send(format!("I got {message}")).await?;
        }
        Ok(())
      })
    }
  }

  struct Runtimeless;

  impl BackgroundTask for Runtimeless {
    type Task =
      Pin<Box<dyn Future<Output = Result<(), async_channel::SendError<String>>> + Send + Sync>>;

    type MessageIn = u32;
    type MessageOut = String;

    fn run(self, message_bus: MessageBus<Self::MessageIn, Self::MessageOut>) -> Self::Task {
      Box::pin(async move {
        while let Ok(message) = message_bus.recv().await {
          message_bus.send(format!("I got {message}")).await?;
        }

        Ok(())
      })
    }
  }

  #[test]
  fn basic_tasks() {
    let mut tasks = BackgroundTasks::with_capacity(Default::default(), 2);

    let task_1 = tasks.register(1, TestTask);
    smol::spawn(async move { task_1.send(123).await }).detach();

    let task_2 = tasks.register(2, TestTask);
    smol::spawn(async move { task_2.send(321).await }).detach();

    smol::block_on(async move {
      let mut got_message_task_1 = false;
      let mut got_message_task_2 = false;

      while let Some((id, update)) = tasks.next().await {
        match (id, update) {
          (1, TaskUpdate::Message(message)) => {
            assert_eq!(message, "I got 123");
            got_message_task_1 = true;
          }
          (1, TaskUpdate::Finished(result)) => {
            assert!(got_message_task_1);
            assert!(result.is_ok())
          }
          (2, TaskUpdate::Message(message)) => {
            assert_eq!(message, "I got 321");
            got_message_task_2 = true;
          }
          (2, TaskUpdate::Finished(result)) => {
            assert!(got_message_task_2);
            assert!(result.is_ok())
          }
          (id, update) => panic!("unexpected message: ({id}, {update:?})"),
        }
      }

      assert!(got_message_task_1);
      assert!(got_message_task_2);
    });
  }

  #[test]
  fn register_typed_task() {
    let mut tasks: tasks::BackgroundTasks<_, _, Result<(), async_channel::SendError<String>>, _> =
      BackgroundTasks::with_capacity(Default::default(), 1);

    let task_1 = tasks.register_typed(TestTask);
    smol::spawn(async move { task_1.send(123).await }).detach();

    smol::block_on(async move {
      let Some((core::any::TypeId { .. }, TaskUpdate::Message(message))) = tasks.next().await
      else {
        panic!("bad task next value");
      };

      assert_eq!(message, "I got 123");

      let Some((core::any::TypeId { .. }, TaskUpdate::Finished(result))) = tasks.next().await
      else {
        panic!("bad task next value");
      };

      assert!(result.is_ok())
    })
  }

  #[test]
  fn runtimeless_task() {
    let mut tasks: tasks::BackgroundTasks<_, _, Result<(), async_channel::SendError<String>>, _> =
      BackgroundTasks::with_capacity(Default::default(), 1);

    let task_1 = tasks.register(1, Runtimeless);
    let _ = smol::block_on(async move { task_1.send(123).await });

    smol::block_on(async move {
      let Some((1, TaskUpdate::Message(message))) = tasks.next().await else {
        panic!("bad task next value");
      };

      assert_eq!(message, "I got 123");

      let Some((1, TaskUpdate::Finished(result))) = tasks.next().await else {
        panic!("bad task next value");
      };

      assert!(result.is_ok())
    })
  }
}
