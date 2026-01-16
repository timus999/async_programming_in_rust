use std::pin::Pin;
use std::sync::LazyLock;
use std::task::{Context, Poll};
use std::time::Duration;
use std::{future::Future, panic::catch_unwind, thread};

use async_task::{Runnable, Task};
use futures_lite::future;

// Creating a simple executor where tasks are queued and run on one thread.
fn spawn_task<F, T>(future: F) -> Task<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    // Lazy initialization
    // The QUEUE is a sender end of a channel, initialized once. It spawns a background thread that loops
    // recieving Runnable's and running them
    static QUEUE: LazyLock<flume::Sender<Runnable>> = LazyLock::new(|| {
        let (tx, rx) = flume::unbounded::<Runnable>();

        thread::spawn(move || {
            while let Ok(runnable) = rx.recv() {
                println!("runnable accepted");
                let _ = catch_unwind(|| runnable.run());
            }
        });
        tx
    });

    // The schedule closure sends runnable to the queue, which the background thread picks up.
    let schedule = |runnable| QUEUE.send(runnable).unwrap();

    // it wraps the future into a Runnable ( which polls it ) and a Task (handle).
    // runnable.schedult() sends it initially to the queue.
    let (runnable, task) = async_task::spawn(future, schedule);
    runnable.schedule();
    println!("Here is the queue count: {:?}", QUEUE.len());
    return task;
}

// Demonstrates polling with artificial delay. The sleep blocks, simulating work, but in real
// async, you'd use non-blocking ops. Waking immediately after Pending ensures quick rescheduling
// (though in this single-thread setup, it queues up).
struct CounterFuture {
    count: u32,
}
impl Future for CounterFuture {
    type Output = u32;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.count += 1;

        println!("Polling with result: {}", self.count);
        std::thread::sleep(Duration::from_secs(1));
        if self.count < 3 {
            cx.waker().wake_by_ref();
            return Poll::Pending;
        } else {
            return Poll::Ready(self.count);
        }
    }
}


// mixing sync blocking in async
async fn async_fn() {
    std::thread::sleep(Duration::from_secs(1));
    println!("async fn");
}

use std::time::Instant;

struct AsyncSleep {
    start_time: Instant,
    duration: Duration,
    
}

impl AsyncSleep {
    fn new(duration: Duration) -> Self {
        Self {
            start_time: Instant::now(),
            duration,
        }
    }
}

impl Future for AsyncSleep {
    type Output = bool;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let elapsed_time = self.start_time.elapsed();
        if elapsed_time >= self.duration {
            Poll::Ready(true)
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}
fn main() {
    let one = CounterFuture { count: 0};
    let two = CounterFuture{ count: 0};
    let t_one = spawn_task(one);
    let t_two = spawn_task(two);
    let t_three = spawn_task(async {
        async_fn().await;
        async_fn().await;
        async_fn().await;
        async_fn().await;
    });
    std::thread::sleep(Duration::from_secs(5));
    println!("before the block");
    future::block_on(t_one);
    future::block_on(t_two);
    future::block_on(t_three);

}
