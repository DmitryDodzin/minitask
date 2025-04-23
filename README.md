# Minitask

Small wrapper for spawning and observing async BackgroundTasks with a "messaging" interface.

The idea is to help with collecting multiple background task updates  

```rust
use minitask::{BackgroundTask, BackgroundTasks, MessageBus, TaskUpdate};
use smol::stream::StreamExt;

struct Ping;

struct Pong;

struct PingPongTask;

impl BackgroundTask for PingPongTask {
  type Task = smol::Task<Result<(), async_channel::SendError<Pong>>>;

  type MessageIn = Ping;
  type MessageOut = Pong;

  fn run(self, message_bus: MessageBus<Self::MessageIn, Self::MessageOut>) -> Self::Task {
    smol::spawn(async move {
      while let Ok(message) = message_bus.recv().await {
        message_bus.send(Pong).await?;
      }
      Ok(())
    })
  }
}

let mut tasks = BackgroundTasks::new();

let ping_pong = tasks.register("ping-pong", PingPongTask);

smol::spawn(async move { ping_pong.send(Ping).await }).detach();

smol::block_on(async move {
  let first_update = tasks.next().await;
  assert!(matches!(
    first_update,
    Some(("ping-pong", TaskUpdate::Message(Pong)))
  ));

  let second_update = tasks.next().await;
  assert!(matches!(
    second_update,
    Some(("ping-pong", TaskUpdate::Finished(Ok(()))))
  ));
});
```

