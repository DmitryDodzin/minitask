#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(not(feature = "std"), no_std)]
#![deny(unused_crate_dependencies)]
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations, rust_2018_idioms)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
mod future;
#[cfg(feature = "alloc")]
mod stream;
#[cfg(any(feature = "alloc", feature = "portable-atomic"))]
mod task;
#[cfg(feature = "alloc")]
mod tasks;

#[cfg(any(feature = "alloc", feature = "portable-atomic"))]
#[cfg_attr(docsrs, doc(cfg(any(feature = "alloc", feature = "portable-atomic"))))]
pub use task::{BackgroundTask, MessageBus};

#[cfg(feature = "alloc")]
#[cfg_attr(docsrs, doc(cfg(feature = "alloc")))]
pub use tasks::{BackgroundTasks, TaskSender, TaskUpdate};

#[cfg(all(test, feature = "alloc"))]
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
  fn basic_task_bounded() {
    let mut tasks = BackgroundTasks::with_capacity(Default::default(), 2);

    let task_1 = tasks.register_bounded(1, TestTask, 1);
    smol::spawn(async move { task_1.send(123).await }).detach();

    let task_2 = tasks.register_bounded(2, TestTask, 1);
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
    let mut tasks = BackgroundTasks::with_capacity(Default::default(), 1);

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
    let mut tasks = BackgroundTasks::with_capacity(Default::default(), 1);

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

#[cfg(all(test, not(feature = "alloc")))]
use smol as _;
