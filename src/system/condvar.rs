/* This file is part of DarkFi (https://dark.fi)
 *
 * Copyright (C) 2020-2023 Dyne.org foundation
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as
 * published by the Free Software Foundation, either version 3 of the
 * License, or (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

use std::{
    future::Future,
    pin::Pin,
    sync::Mutex,
    task::{Context, Poll, Waker},
};

/// Condition variable which allows a task to block until woken up
pub struct CondVar {
    state: Mutex<CondVarState>,
}

struct CondVarState {
    is_awake: bool,
    waker: Option<Waker>,
}

impl CondVar {
    pub fn new() -> Self {
        Self { state: Mutex::new(CondVarState { is_awake: false, waker: None }) }
    }

    /// Wakeup the waiting task. Subsequent calls to this do nothing until `wait()` is called.
    pub fn notify(&self) {
        let mut state = self.state.lock().unwrap();
        state.is_awake = true;
        if let Some(waker) = state.waker.take() {
            waker.wake()
        }
    }

    /// Reset the condition variable and wait for a notification
    pub fn wait(&self) -> CondVarWait {
        CondVarWait { state: &self.state }
    }

    /// Reset self ready to wait() again.
    /// The reason this is separate from `wait()` is that usually
    /// on the first `wait()` we want to catch any `notify()` calls that
    /// happened before we started. For example,
    /// ```rust
    /// loop {
    ///     // Wait for signal
    ///     cv.wait().await;
    ///
    ///     // Do stuff...
    ///
    ///     cv.reset();
    /// }
    /// ```
    pub fn reset(&self) {
        let mut state = self.state.lock().unwrap();
        state.is_awake = false;
    }
}

impl Default for CondVar {
    fn default() -> Self {
        Self::new()
    }
}

pub struct CondVarWait<'a> {
    state: &'a Mutex<CondVarState>,
}

impl<'a> Future for CondVarWait<'a> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.state.lock().unwrap();

        // Avoid cloning wherever possible.
        let new_waker = match state.waker.take() {
            Some(waker) => {
                let cx_waker = cx.waker();
                if cx_waker.will_wake(&waker) {
                    waker
                } else {
                    cx_waker.clone()
                }
            }
            None => cx.waker().clone(),
        };
        state.waker = Some(new_waker);

        match state.is_awake {
            true => Poll::Ready(()),
            false => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smol::Executor;
    use std::sync::Arc;

    #[test]
    fn condvar_test() {
        let executor = Arc::new(Executor::new());
        let executor_ = executor.clone();
        smol::block_on(executor.run(async move {
            let cv = Arc::new(CondVar::new());

            let cv_ = cv.clone();
            executor_
                .spawn(async move {
                    // Waits here until notify() is called
                    cv_.wait().await;
                })
                .detach();

            // Allow above code to continue
            cv.notify();
        }))
    }

    #[test]
    fn condvar_reset() {
        let executor = Arc::new(Executor::new());
        let executor_ = executor.clone();
        smol::block_on(executor.run(async move {
            let cv = Arc::new(CondVar::new());

            let cv_ = cv.clone();
            executor_
                .spawn(async move {
                    cv_.wait().await;
                })
                .detach();

            // #1 send signal
            cv.notify();
            // Multiple calls to notify do nothing until we call reset()
            cv.notify();

            // Without calling reset(), then the wait() will return instantly
            cv.reset();

            let cv_ = cv.clone();
            executor_
                .spawn(async move {
                    cv_.wait().await;
                })
                .detach();

            // #2 send signal again
            cv.notify();
        }))
    }
}
