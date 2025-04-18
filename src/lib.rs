//! # Minitask
//!
//! Small wrapper for spawning and observing [`BackgroundTask`]s with a "messaging" interface,
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
//!   type Error = async_channel::SendError<Pong>;
//!
//!   type MessageIn = Ping;
//!   type MessageOut = Pong;
//!
//!   fn run(
//!     self,
//!     message_bus: MessageBus<Self::MessageIn, Self::MessageOut>,
//!   ) -> smol::Task<Result<(), Self::Error>> {
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

use async_channel::{Receiver, Sender};
use async_task::Task;

mod future;
mod stream;
mod tasks;

pub use tasks::{BackgroundTasks, TaskSender, TaskUpdate};

pub trait BackgroundTask {
  type Error;

  /// Incoming message type
  type MessageIn;

  /// Outgoing message type
  type MessageOut;

  /// Start the task with the provided [`MessageBus`]
  fn run(
    self,
    message_bus: MessageBus<Self::MessageIn, Self::MessageOut>,
  ) -> Task<Result<(), Self::Error>>;
}

#[derive(Clone)]
pub struct MessageBus<MessageIn, MessageOut> {
  tx: Sender<MessageOut>,
  rx: Receiver<MessageIn>,
}

impl<MessageIn, MessageOut> MessageBus<MessageIn, MessageOut> {
  fn new(tx: Sender<MessageOut>, rx: Receiver<MessageIn>) -> Self {
    MessageBus { tx, rx }
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

  use smol::stream::StreamExt;

  use super::*;

  struct TestTask;

  impl BackgroundTask for TestTask {
    type Error = async_channel::SendError<String>;

    type MessageIn = u32;
    type MessageOut = String;

    fn run(
      self,
      message_bus: MessageBus<Self::MessageIn, Self::MessageOut>,
    ) -> Task<Result<(), Self::Error>> {
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

    let task_2 = tasks.register(3, TestTask);
    smol::spawn(async move {
      let _ = task_2.send(321).await;
      task_2.send(321).await
    })
    .detach();

    smol::block_on(async move {
      while let Some((id, message)) = tasks.next().await {
        println!("{id}: {message:?}");
      }
    });
  }
}
