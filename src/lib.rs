

#![feature(fnbox)]

extern crate spin;
extern crate num_cpus;

use std::sync::Arc;
use std::thread::{self, Thread};
use std::boxed::FnBox;
use std::mem;
use std::error::Error;
use std::fmt::{self, Display};
use spin::Mutex;


mod threadpool;


pub type AwaitResult <T> = Result<T, AwaitError>;


pub fn promise_future <T> () -> (Promise<T>, Future<T>)
    where T : Send + 'static
{
    let store = Arc::new(Mutex::new((PromiseValue::Impending, None, None)));

    (Promise(Some(store.clone()), false), Future(FutureInner::Impending(store)))
}
fn continue_future <T> () -> (Promise<T>, Future<T>)
    where T : Send + 'static
{
    let store = Arc::new(Mutex::new((PromiseValue::Impending, None, None)));

    (Promise(Some(store.clone()), true), Future(FutureInner::Impending(store)))
}


#[must_use]
pub struct Promise <T> (Option<Arc<Mutex<(PromiseValue<T>, Option<Box<FnBox(T) + Send + 'static>>, Option<Thread>)>>>, bool)
    where T : Send + 'static;
impl <T> Promise<T>
    where T : Send + 'static
{
    pub fn fulfill (mut self, val : T)
    {
        let store = self.0.take().unwrap();
        let mut lock = store.lock();

        if let Some(cont) = lock.1.take() {
            if self.1 {
                cont.call_box((val,));
            } else {
                thread::spawn(move|| cont.call_box((val,)));
            }
        } else {
            lock.0 = PromiseValue::Present(val);
            if let Some(ref thread) = lock.2 {
                thread.unpark();
            }
        }
    }
}
impl <T, E> Promise<Result<T, E>>
    where T : Send + 'static, E : Send + 'static
{
    #[inline]
    pub fn ok (self, val : T)
    {
        self.fulfill(Ok(val));
    }
    #[inline]
    pub fn err (self, err : E)
    {
        self.fulfill(Err(err));
    }
}
impl <T> Drop for Promise<T>
    where T : Send + 'static
{
    fn drop (&mut self)
    {
        if let Some(store) = self.0.take() {
            let mut lock = store.lock();
            lock.0 = PromiseValue::Broken;
            if let Some(ref thread) = lock.2 {
                thread.unpark();
            }
        }
    }
}

pub enum PromiseValue <T>
    where T : Send + 'static
{
    Impending,
    Present (T),
    Broken,
}
impl <T> PromiseValue<T>
    where T : Send + 'static
{
    pub fn take (&mut self) -> PromiseValue<T>
    {
        let mut pv = PromiseValue::Impending;
        mem::swap(self, &mut pv);
        pv
    }
}


pub struct Future <T> (FutureInner<T>)
    where T : Send + 'static;
enum FutureInner <T>
    where T : Send + 'static
{
    Impending (Arc<Mutex<(PromiseValue<T>, Option<Box<FnBox(T) + Send + 'static>>, Option<Thread>)>>),
    Present   (T),
    Broken,
}
impl <T> Future<T>
    where T : Send + 'static
{
    #[inline]
    pub fn present (val : T) -> Future<T> { Future(FutureInner::Present(val)) }
    

    pub fn poll (&mut self) -> FutureState
    {
        let inner = {
            let store = match self.0 {
                FutureInner::Impending(ref mut store) => store,
                FutureInner::Present(..) => return FutureState::Present,
                FutureInner::Broken(..) => return FutureState::Broken,
            };

            match store.lock().0.take() {
                PromiseValue::Impending => return FutureState::Impending,
                PromiseValue::Present(val) => FutureInner::Present(val),
                PromiseValue::Broken => FutureInner::Broken,
            }
        };
        self.0 = inner;

        match self.0 {
            FutureInner::Impending(..) => FutureState::Impending,
            FutureInner::Present(..) => FutureState::Present,
            FutureInner::Broken => FutureState::Broken,
        }
    }


    pub fn await (self) -> AwaitResult<T>
    {
        match self.0 {
            FutureInner::Impending(store) => {
                loop {
                    {
                        let mut lock = store.lock();
                        match lock.0.take() {
                            PromiseValue::Impending => (),
                            PromiseValue::Present(val) => return Ok(val),
                            PromiseValue::Broken => return Err(AwaitError::Broken),
                        }
                        if let None = lock.2 {
                            lock.2 = Some(thread::current());
                        }
                    }
                    thread::park();
                }
            },
            FutureInner::Present(val) => Ok(val),
            FutureInner::Broken => Err(AwaitError::Broken),
        }
    }

    pub fn then <F, U> (self, cont : F) -> Future<U>
        where F : FnOnce(T) -> U + Send + 'static, U : Send + 'static
    {
        match self.0 {
            FutureInner::Impending(store) => {
                let mut lock = store.lock();

                match lock.0.take() {
                    PromiseValue::Impending => {
                        let (p, f) = continue_future();
                        lock.1 = Some(Box::new(move|val| p.fulfill(cont(val))));
                        f
                    },
                    PromiseValue::Present(val) => {
                        let (p, f) = continue_future();
                        thread::spawn(move|| p.fulfill(cont(val)));
                        f
                    },
                    PromiseValue::Broken => Future(FutureInner::Broken),
                }
            },
            FutureInner::Present(val) => {
                let (p, f) = continue_future();
                thread::spawn(move|| p.fulfill(cont(val)));
                f
            },
            FutureInner::Broken => Future(FutureInner::Broken),
        }
    }
}
impl <T, E> Future<Result<T, E>>
    where T : Send + 'static, E : Send + From<AwaitError> + 'static
{
    pub fn result (self) -> Result<T, E>
    {
        match self.await() {
            Ok(res) => res,
            Err(e) => Err(e.into()),
        }
    }

    pub fn and_then <F, U, G> (self, cont : F) -> Future<Result<U, G>>
        where F : FnOnce(T) -> Result<U, G> + Send + 'static, U : Send + 'static, G : Send + From<E> + 'static
    {
        self.then(|res| match res {
            Ok(val) => cont(val),
            Err(e) => Err(e.into()),
        })
    }
}


#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum FutureState
{
    Impending,
    Present,
    Broken,
}


#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum AwaitError
{
    Broken,
}
impl Error for AwaitError
{
    fn description (&self) -> &str
    {
        match *self {
            AwaitError::Broken => "Broken future",
        }

    }

    fn cause (&self) -> Option<&Error> { None }
}
impl Display for AwaitError
{
    fn fmt (&self, fmt : &mut fmt::Formatter) -> ::std::result::Result<(), ::std::fmt::Error>
    {
        write!(fmt, "AwaitError: {}", self.description())
    }
}





