

extern crate alternate_future;

use std::thread;
use std::cell::Cell;
use alternate_future::{promise_future, spawn_future, Future};


#[test]
fn present ()
{
    let f = Future::present(5);
    let i = f.await().unwrap();

    assert_eq!(i, 5);
}


#[test]
fn fulfill ()
{
    let (p, f) = promise_future();

    thread::spawn(move|| {
        thread::sleep_ms(100);
        p.fulfill(5);
    });

    let i : i32 = f.await().unwrap();

    assert_eq!(i, 5);
}


#[test]
fn broken ()
{
    let (p, f) = promise_future::<i32>();

    thread::spawn(move|| {
        thread::sleep_ms(100);
        drop(p);
    });

    assert!(f.await().is_err());
}


#[test]
fn early ()
{
    let (p, f) = promise_future();

    thread::spawn(move|| {
        p.fulfill(5);
    });

    thread::sleep_ms(100);
    let i : i32 = f.await().unwrap();

    assert_eq!(i, 5);
}


thread_local!{
    static KEY : Cell<i32> = Cell::new(0)
}

#[test]
fn chain ()
{
    let (p, f) = promise_future();

    thread::spawn(move|| {
        KEY.with(|k| k.set(1));
        thread::sleep_ms(100);
        p.fulfill(5);
    });

    let i : i32 = f.then(|i| {
        KEY.with(|k| assert_eq!(k.get(), 0));
        i * 2
    }).await().unwrap();

    assert_eq!(i, 10);
}


#[test]
fn spawn ()
{
    assert_eq!(
        spawn_future(|| {
            thread::sleep_ms(100);
            12
        }).await().unwrap(), 
        12
    );
}


