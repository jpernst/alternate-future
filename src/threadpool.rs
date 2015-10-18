// Copyright 2015 Jameson Ernst
// Copyright 2014 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! Abstraction of a thread pool for basic parallelism.
//! Modified to prevent blocking: If no threads are
//! currently available to execute the job, a new thread
//! will be created on demand.
//! Modified to 


use std::sync::mpsc::{channel, Sender, Receiver};
use std::sync::Arc;
use std::thread;
use spin::Mutex;


trait FnBox
{
    fn call_box(self: Box<Self>);
}
impl<F: FnOnce()> FnBox for F
{
    fn call_box(self: Box<F>)
    {
        (*self)()
    }
}

type Thunk<'a> = Box<FnBox + Send + 'a>;


struct Sentinel<'a>
{
    jobs: &'a Arc<Mutex<Receiver<Thunk<'static>>>>,
    active: bool
}
impl<'a> Sentinel<'a>
{
    fn new(jobs: &'a Arc<Mutex<Receiver<Thunk<'static>>>>) -> Sentinel<'a> {
        Sentinel {
            jobs: jobs,
            active: true
        }
    }

    fn cancel(mut self) {
        self.active = false;
    }
}
impl<'a> Drop for Sentinel<'a>
{
    fn drop(&mut self)
    {
        if self.active {
            spawn_in_pool(self.jobs.clone())
        }
    }
}


#[derive(Clone)]
pub struct ThreadPool
{
    jobs: Sender<Thunk<'static>>
}
impl ThreadPool
{
    pub fn new(threads: usize) -> ThreadPool
    {
        assert!(threads >= 1);

        let (tx, rx) = channel::<Thunk<'static>>();
        let rx = Arc::new(Mutex::new(rx));

        for _ in 0..threads {
            spawn_in_pool(rx.clone());
        }

        ThreadPool { jobs: tx }
    }

    pub fn execute<F>(&self, job: F)
        where F : FnOnce() + Send + 'static
    {
        self.jobs.send(Box::new(move || job())).unwrap();
    }
}

fn spawn_in_pool(jobs: Arc<Mutex<Receiver<Thunk<'static>>>>)
{
    thread::spawn(move || {
        let sentinel = Sentinel::new(&jobs);

        loop {
            let message = {
                let lock = jobs.lock();
                lock.recv()
            };

            match message {
                Ok(job) => job.call_box(),
                Err(..) => break,
            }
        }

        sentinel.cancel();
    });
}


