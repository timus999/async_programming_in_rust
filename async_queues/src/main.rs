use std::pin::Pin;
use std::sync::LazyLock;
use std::task::{Context, Poll};
use std::time::Duration;
use std::{future::Future, panic::catch_unwind, thread};

use async_task::{Runnable, Task};
use flume::{Receiver, Sender};
use futures_lite::future;

// Creating our own macro for spawing task so that developer does not stress over the order
macro_rules! spawn_task {
    ($future:expr) => {
        spawn_task!($future, FutureType::Low)
    };
    ($future:expr, $order:expr) => {
        spawn_task($future, $order)
    };
}

// creating our own join macro
macro_rules! join {
    ($($future:expr),*) => {
        {
            let mut results = Vec::new();
            $(
                results.push(future::block_on($future));
            )*
            results
        }
    }
}

// Error can occur when joining
// so we'll create try_join macro to handle error
macro_rules! try_join {
    ($($future:expr),*) => {
        {
            let mut results = Vec::new();
            $(
                let result = catch_unwind(|| future::block_on($future));
                results.push(result);
            )*
            results
        }
    }
}


// creating runtime
struct Runtime {
    high_num: usize,
    low_num: usize,
}

impl Runtime {
    pub fn new() -> Self {
        let num_cores = std::thread::available_parallelism().unwrap().get();
        Self {
            high_num: num_cores - 2,
            low_num: 1,
        }
        
    }

    pub fn with_high_num(mut self, num: usize) -> Self {
        self.high_num = num;
        self
    }
    pub fn with_low_num(mut self, num: usize) -> Self {
        self.low_num = num;
        self
    }

    pub fn run(&self) {
        unsafe {
        std::env::set_var("HIGH_NUM", self.high_num.to_string());
        std::env::set_var("LOW_NUM", self.low_num.to_string());

        }
        let high = spawn_task!(async {}, FutureType::High);
        let low = spawn_task!(async {}, FutureType::Low);
        join!(high, low);
    }
}
// Creating a simple executor where tasks are queued and run on one thread.
fn spawn_task<F, T>(future: F, order: FutureType) -> Task<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    static HIGH_CHANNEL: LazyLock<(Sender<Runnable>, Receiver<Runnable>)> =
        LazyLock::new(|| flume::unbounded::<Runnable>());
    static LOW_CHANNEL: LazyLock<(Sender<Runnable>, Receiver<Runnable>)> =
        LazyLock::new(|| flume::unbounded::<Runnable>());
    // Lazy initialization
    // The QUEUE is a sender end of a channel, initialized once. It spawns a background thread that loops
    // recieving Runnable's and running them
    static HIGHQUEUE: LazyLock<flume::Sender<Runnable>> = LazyLock::new(|| {
        let high_num = std::env::var("HIGH_NUM").unwrap().parse::<usize>().unwrap();
        for _ in 0..high_num {
            let high_reciever = HIGH_CHANNEL.1.clone();
            let low_reciever = LOW_CHANNEL.1.clone();
            thread::spawn(move || loop {
                match high_reciever.try_recv() {
                    Ok(runnable) => {
                        let _ = catch_unwind(|| runnable.run());
                    }
                    Err(_) => match low_reciever.try_recv() {
                        Ok(runnable) => {
                            let _ = catch_unwind(|| runnable.run());
                        }
                        Err(_) => {
                            thread::sleep(Duration::from_millis(100));
                        }
                    },
                };
            });
        }

        HIGH_CHANNEL.0.clone()
    });
    static LOWQUEUE: LazyLock<flume::Sender<Runnable>> = LazyLock::new(|| {

        
        let low_num = std::env::var("LOW_NUM").unwrap().parse::<usize>().unwrap();
        for _ in 0..low_num {
        let low_reciever = LOW_CHANNEL.1.clone();
        let high_reciever = HIGH_CHANNEL.1.clone();

        thread::spawn(move || loop {
            match low_reciever.try_recv() {
                Ok(runnable) => {
                    let _ = runnable.run();
                }
                Err(_) => match high_reciever.try_recv() {
                    Ok(runnable) => {
                        let _ = runnable.run();
                    }
                    Err(_) => {
                        thread::sleep(Duration::from_millis(100));
                    }
                },
            }
        });
        }

        LOW_CHANNEL.0.clone()
    });

    // The schedule closure sends runnable to the queue, which the background thread picks up.
    let schedule_high = |runnable| HIGHQUEUE.send(runnable).unwrap();
    let schedule_low = |runnable| LOWQUEUE.send(runnable).unwrap();

    // it wraps the future into a Runnable ( which polls it ) and a Task (handle).
    // runnable.schedult() sends it initially to the queue.
    let schedule = match order {
        FutureType::High => schedule_high,
        FutureType::Low => schedule_low,
    };
    let (runnable, task) = async_task::spawn(future, schedule);

    runnable.schedule();
    return task;
}

#[derive(Clone, Debug, Copy)]
enum FutureType {
    High,
    Low,
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

// use std::time::Instant;

// struct AsyncSleep {
//     start_time: Instant,
//     duration: Duration,
// }

// impl AsyncSleep {
//     fn new(duration: Duration) -> Self {
//         Self {
//             start_time: Instant::now(),
//             duration,
//         }
//     }
// }

// impl Future for AsyncSleep {
//     type Output = bool;
//     fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
//         let elapsed_time = self.start_time.elapsed();
//         if elapsed_time >= self.duration {
//             Poll::Ready(true)
//         } else {
//             cx.waker().wake_by_ref();
//             Poll::Pending
//         }
//     }
// }


// Creating Background process
struct BackgroundFuture;

impl Future for BackgroundFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        println!("background process running");
        std::thread::sleep(Duration::from_secs(1));
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}
fn main() {
    Runtime::new().with_low_num(2).with_high_num(4).run();
    let _background = spawn_task!(BackgroundFuture{});
    let one = CounterFuture { count: 0 };
    let two = CounterFuture { count: 0 };
    let t_one = spawn_task!(one, FutureType::High);
    let t_two = spawn_task!(two);
    let t_three = spawn_task!(async_fn());
    let t_four = spawn_task!(async {
        async_fn().await;
        async_fn().await;
    }, FutureType::High);

    let _outcome: Vec<u32> = join!(t_one, t_two);
    let _outcome_two: Vec<()> = join!(t_four, t_three);
}
