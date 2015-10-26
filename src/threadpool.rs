// Copyright (c) 2014 The Rust Project Developers
//
// Permission is hereby granted, free of charge, to any
// person obtaining a copy of this software and associated
// documentation files (the "Software"), to deal in the
// Software without restriction, including without
// limitation the rights to use, copy, modify, merge,
// publish, distribute, sublicense, and/or sell copies of
// the Software, and to permit persons to whom the Software
// is furnished to do so, subject to the following
// conditions:
//
// The above copyright notice and this permission notice
// shall be included in all copies or substantial portions
// of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
// ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
// TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
// PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
// SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
// OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
// IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.


use std::sync::mpsc::{sync_channel, SyncSender, Receiver, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;


trait FnBox
{
    fn call_box (self : Box<Self>);
}
impl <F> FnBox for F
    where F : FnOnce()
{
    fn call_box (self : Box<F>)
    {
        (*self)()
    }
}

type Thunk <'a> = Box<FnBox + Send + 'a>;


struct Sentinel<'a>
{
    jobs: &'a Arc<Mutex<Receiver<Thunk<'static>>>>,
    active: bool
}
impl<'a> Sentinel<'a>
{
    fn new (jobs : &'a Arc<Mutex<Receiver<Thunk<'static>>>>) -> Sentinel<'a>
    {
        Sentinel {
            jobs: jobs,
            active: true
        }
    }

    fn cancel (mut self) {
        self.active = false;
    }
}
impl<'a> Drop for Sentinel<'a>
{
    fn drop (&mut self)
    {
        if self.active {
            spawn_in_pool(self.jobs.clone())
        }
    }
}


#[derive(Clone)]
pub struct ThreadPool
{
    jobs: SyncSender<Thunk<'static>>
}
impl ThreadPool
{
    pub fn new (threads : usize) -> ThreadPool
    {
        assert!(threads >= 1);

        let (tx, rx) = sync_channel::<Thunk<'static>>(0);
        let rx = Arc::new(Mutex::new(rx));

        for _ in 0..threads {
            spawn_in_pool(rx.clone());
        }

        ThreadPool { jobs: tx }
    }

    pub fn execute <F> (&self, job : F)
        where F : FnOnce() + Send + 'static
    {
        let job = Box::new(move|| job());
        match self.jobs.try_send(job) {
            Err(TrySendError::Full(job)) => { thread::spawn(|| job.call_box()); },
            Err(..) => panic!(),
            Ok(..) => (),
        }
    }
}

fn spawn_in_pool (jobs : Arc<Mutex<Receiver<Thunk<'static>>>>)
{
    thread::spawn (move|| {
        let sentinel = Sentinel::new(&jobs);

        loop {
            let job = {
                let lock = jobs.lock().unwrap();
                lock.recv()
            };
            match job {
                Ok(job) => job.call_box(),
                Err(..) => break,
            }
        }

        sentinel.cancel();
    });
}


