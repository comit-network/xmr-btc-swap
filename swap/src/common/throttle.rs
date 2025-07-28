// copied from: https://github.com/cargo-crates/fns
// MIT License

use std::pin::Pin;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{self, /* SystemTime, UNIX_EPOCH, */ Duration};

pub fn throttle<F, T>(closure: F, delay: Duration) -> Throttle<T>
where
    F: Fn(T) -> () + Send + Sync + 'static,
    T: Send + Sync + 'static,
{
    let (sender, receiver) = mpsc::channel();
    let sender = Arc::new(Mutex::new(sender));
    let throttle_config = Arc::new(Mutex::new(ThrottleConfig {
        closure: Box::pin(closure),
        delay,
    }));

    let dup_throttle_config = throttle_config.clone();
    let throttle = Throttle {
        sender: Some(sender),
        thread: Some(std::thread::spawn(move || {
            let throttle_config = dup_throttle_config;
            let mut current_param = None; // 最后被保存为执行的参数
            let mut closure_time = None; // 闭包最后执行时间
            loop {
                if current_param.is_none() {
                    let message = receiver.recv();
                    let now = time::Instant::now();
                    match message {
                        Ok(param) => {
                            if let Some(param) = param {
                                let throttle_config = throttle_config.lock().unwrap();
                                if closure_time.is_none()
                                    || now.duration_since(closure_time.unwrap())
                                        >= throttle_config.delay
                                {
                                    current_param = None;
                                    closure_time = Some(now);
                                    (*throttle_config.closure)(param);
                                } else {
                                    current_param = Some(param);
                                }
                            } else {
                                current_param = None;
                            }
                        }
                        Err(_) => {
                            break;
                        }
                    }
                } else {
                    let message = receiver.recv_timeout((*throttle_config.lock().unwrap()).delay);
                    let now = time::Instant::now();
                    match message {
                        Ok(param) => {
                            if let Some(param) = param {
                                let throttle_config = throttle_config.lock().unwrap();
                                if closure_time.is_none()
                                    || now.duration_since(closure_time.unwrap())
                                        >= throttle_config.delay
                                {
                                    (*throttle_config.closure)(param);
                                    current_param = None;
                                    closure_time = Some(now);
                                } else {
                                    current_param = Some(param);
                                }
                            } else {
                                current_param = None;
                            }
                        }
                        Err(err) => {
                            match err {
                                mpsc::RecvTimeoutError::Timeout => {
                                    if let Some(param) = current_param.take() {
                                        (throttle_config.lock().unwrap().closure)(param);
                                        current_param = None;
                                        closure_time = None; // 超时执行为额外的执行, 不影响的下一次执行
                                    }
                                }
                                mpsc::RecvTimeoutError::Disconnected => {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        })),
        throttle_config,
    };
    throttle
}

struct ThrottleConfig<T> {
    closure: Pin<Box<dyn Fn(T) -> () + Send + Sync + 'static>>,
    delay: Duration,
}
impl<T> Drop for ThrottleConfig<T> {
    fn drop(&mut self) {
        tracing::debug!("drop ThrottleConfig {:?}", format!("{:p}", self));
    }
}

#[allow(dead_code)]
pub struct Throttle<T> {
    sender: Option<Arc<Mutex<mpsc::Sender<Option<T>>>>>,
    thread: Option<std::thread::JoinHandle<()>>,
    throttle_config: Arc<Mutex<ThrottleConfig<T>>>,
}
impl<T> Throttle<T> {
    pub fn call(&self, param: T) {
        self.sender
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .send(Some(param))
            .unwrap();
    }
    pub fn terminate(&self) {
        self.sender
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .send(None)
            .unwrap();
    }
}
impl<T> Drop for Throttle<T> {
    fn drop(&mut self) {
        self.terminate();
        tracing::debug!("drop Throttle {:?}", format!("{:p}", self));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let effect_run_times = Arc::new(Mutex::new(0));
        let param = Arc::new(Mutex::new(0));
        let dup_effect_run_times = effect_run_times.clone();
        let dup_param = param.clone();
        let throttle_fn = throttle(
            move |param| {
                *dup_effect_run_times.lock().unwrap() += 1;
                *dup_param.lock().unwrap() = param;
            },
            std::time::Duration::from_millis(100),
        );
        {
            throttle_fn.call(1);
            throttle_fn.call(2);
            throttle_fn.call(3);
            std::thread::sleep(std::time::Duration::from_millis(200));
            assert_eq!(*effect_run_times.lock().unwrap(), 2); // delay后执行最有一个参数
            assert_eq!(*param.lock().unwrap(), 3);
        }

        {
            throttle_fn.call(4);
            std::thread::sleep(std::time::Duration::from_millis(200));
            assert_eq!(*effect_run_times.lock().unwrap(), 3);
            assert_eq!(*param.lock().unwrap(), 4);
        }

        {
            throttle_fn.call(5);
            throttle_fn.call(6);
            throttle_fn.terminate(); // 终止最后一次执行
            std::thread::sleep(std::time::Duration::from_millis(200));
            assert_eq!(*effect_run_times.lock().unwrap(), 4);
            assert_eq!(*param.lock().unwrap(), 5);
        }
    }
}
