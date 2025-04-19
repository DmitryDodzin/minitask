//! # Minitask
//!
//! Small wrapper for spawning and observing async [`BackgroundTask`]s with a "messaging" interface,
//! ```rust
//! use minitask::{BackgroundTask, BackgroundTasks, MessageBus, TaskUpdate};
//! use smol::stream::StreamExt;
//!
//! struct Ping;
//!
//! struct Pong;
//!
//! struct PingPongTask;
//!
//! impl BackgroundTask for PingPongTask {
//!   type Task = smol::Task<Result<(), Self::Error>>;
//!   type Error = async_channel::SendError<Pong>;
//!
//!   type MessageIn = Ping;
//!   type MessageOut = Pong;
//!
//!   fn run(self, message_bus: MessageBus<Self::MessageIn, Self::MessageOut>) -> Self::Task {
//!     smol::spawn(async move {
//!       while let Ok(message) = message_bus.recv().await {
//!         message_bus.send(Pong).await?;
//!       }
//!       Ok(())
//!     })
//!   }
//! }
//!
//! let mut tasks = BackgroundTasks::new();
//!
//! let ping_pong = tasks.register("ping-pong", PingPongTask);
//!
//! smol::spawn(async move { ping_pong.send(Ping).await }).detach();
//!
//! smol::block_on(async move {
//!   let first_update = tasks.next().await;
//!   assert!(matches!(
//!     first_update,
//!     Some(("ping-pong", TaskUpdate::Message(Pong)))
//!   ));
//!
//!   let second_update = tasks.next().await;
//!   assert!(matches!(
//!     second_update,
//!     Some(("ping-pong", TaskUpdate::Finished(Ok(()))))
//!   ));
//! });
//! ```

#![deny(unsafe_code)]
#![deny(unused_crate_dependencies)]
#![no_std]

extern crate alloc;

use async_channel::{Receiver, Sender};

mod future;
mod stream;
mod tasks;

pub use tasks::{BackgroundTasks, TaskSender, TaskUpdate};

pub trait BackgroundTask {
  type Task: Future<Output = Result<(), Self::Error>>;
  type Error;

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

  use alloc::{format, string::String};

  use smol::stream::StreamExt;

  use super::*;

  struct TestTask;

  impl BackgroundTask for TestTask {
    type Task = smol::Task<Result<(), Self::Error>>;
    type Error = async_channel::SendError<String>;

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

  #[test]
  fn basic_tasks() {
    let mut tasks = BackgroundTasks::new();

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
    });
  }
}
