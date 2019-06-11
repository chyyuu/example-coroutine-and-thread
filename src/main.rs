#![feature(asm)]
#![feature(naked_functions)]
#![feature(futures_api)]
use std::ptr;
use std::collections::HashSet;

const DEFAULT_STACK_SIZE: usize = 1024 * 1024* 2;
const MAX_THREADS: usize = 4;
static mut RUNTIME: usize = 0;

pub struct Runtime {
    threads: Vec<Thread>,
    incoming: HashSet<usize>,
    pending: HashSet<usize>,
    current: usize,
}


#[derive(PartialEq, Eq, Debug)]
enum State {
    Available,
    Running,
    Ready,
}

struct Thread {
    id: usize,
    stack: Vec<u8>,
    ctx: ThreadContext,
    state: State,
    task: Option<Box<dyn Future<Output = ()>>>,
}

#[derive(Debug, Default)]
#[repr(C)] 
struct ThreadContext {
    rsp: u64,
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    rbx: u64,
    rbp: u64,
    thread_ptr: u64,
}

impl Thread {
    fn new(id: usize) -> Self {
        Thread {
            id,
            stack: vec![0_u8; DEFAULT_STACK_SIZE],
            ctx: ThreadContext::default(),
            state: State::Available,
            task: None,
        }
    }
}

impl Runtime {
    pub fn new() -> Self {
        let base_thread = Thread {
            id: 0,
            stack: vec![0_u8; DEFAULT_STACK_SIZE],
            ctx: ThreadContext::default(),
            state: State::Running,
            task: None,
        };

        let mut threads = vec![base_thread];
        // since we store code seperately we need to store a pointer to our base thread, it's OK to do here
        threads[0].ctx.thread_ptr = &threads[0] as *const Thread as u64;

        // we could store pointers to our threads here as well since we initialize all of them here but it's easier
        // to follow the logic if we do it when we spawn a thread.
        let mut available_threads: Vec<Thread> = (1..MAX_THREADS).map(|i| Thread::new(i)).collect();
        threads.append(&mut available_threads);

       Runtime {
            threads,
            incoming: HashSet::new(),
            pending: HashSet::new(),
            current: 0,
        }
    }

    pub fn init(&self) {
        unsafe {
            let r_ptr: *const Runtime = self;
            RUNTIME = r_ptr as usize;
        }
    }

    pub fn run(&mut self) -> ! {
        while self.t_yield() {}
        std::process::exit(0);
    }

    fn t_return(&mut self) {
        if self.current != 0 {
            self.threads[self.current].state = State::Available;
            self.t_yield();
        }
    }
    
    fn t_yield(&mut self) -> bool {
        let mut pos = self.current;
        while self.threads[pos].state != State::Ready {
            pos += 1;
            if pos == self.threads.len() {
                pos = 0;
            }
            if pos == self.current {
                return false;
            }
        }

        // check if the task is parked, if it is then don't poll it
        // if it's not parked then run it

        if self.threads[self.current].state != State::Available {
            self.threads[self.current].state = State::Ready;
        }

        self.threads[pos].state = State::Running;
        let old_pos = self.current;
        self.current = pos;

        unsafe {
            switch(&mut self.threads[old_pos].ctx, &self.threads[pos].ctx);
        }

        true
    }

    pub fn spawn<F>(&mut self, f: F)
    where F: Future
     {
        let available = self
            .threads
            .iter_mut()
            .find(|t| t.state == State::Available)
            .expect("no available thread.");

        let size = available.stack.len();
        let s_ptr = available.stack.as_mut_ptr();

        // lets put our Fn() trait object on the heap and store it in our thread for now
        available.task = Some(Box::new(f));
        // we needtaskirect reference to this thread to run the code so we need this additional
        // context
        available.ctx.thread_ptr = available as *const Thread as u64;

        unsafe {
            ptr::write(s_ptr.offset((size - 8) as isize) as *mut u64, guard as u64);
            ptr::write(s_ptr.offset((size - 16) as isize) as *mut u64, call as u64);
            available.ctx.rsp = s_ptr.offset((size - 16) as isize) as u64;
        }
        available.state = State::Ready;
    }
}

fn call(thread: u64) {
        let thread = unsafe {&*(thread as *const Thread)};
            if let Some(f) = &thread.task {
            match f.poll() {
                Poll::Pending => // move task to pending queue preventing it from getting polled again
                                 // give a waker to the task so it will call it when ready
                Poll::Ready => // the future resolved successfully
            }
        }
}

#[cfg_attr(any(target_os="windows", target_os="linux"), naked)]
fn guard() {
    unsafe {
        let rt_ptr = RUNTIME as *mut Runtime;
        let rt = &mut *rt_ptr;
        println!("THREAD {} FINISHED.", rt.threads[rt.current].id);
        rt.t_return();
    };
}

pub fn yield_thread() {
    unsafe {
        let rt_ptr = RUNTIME as *mut Runtime;
        (*rt_ptr).t_yield();
    };
}

// see: https://github.com/rust-lang/rfcs/blob/master/text/1201-naked-fns.md
// we don't have to store the code when we switch out of the thread but we need to
// provide a pointer to it when we switch to a thread.
// The `%rdi` register stores the first parameter to the function
#[naked]
#[cfg(not(target_os="windows"))]
unsafe fn switch(old: *mut ThreadContext, new: *const ThreadContext) {
    asm!("
        mov     %rsp, 0x00($0)
        mov     %r15, 0x08($0)
        mov     %r14, 0x10($0)
        mov     %r13, 0x18($0)
        mov     %r12, 0x20($0)
        mov     %rbx, 0x28($0)
        mov     %rbp, 0x30($0)

        mov     0x00($1), %rsp
        mov     0x08($1), %r15
        mov     0x10($1), %r14
        mov     0x18($1), %r13
        mov     0x20($1), %r12
        mov     0x28($1), %rbx
        mov     0x30($1), %rbp
        mov     0x38($1), %rdi
        ret
        "
    : "=*m"(old)
    : "r"(new)
    :
    : "alignstack" 
    );
}

/// Windows uses the `rcx` register for the first parameter.
/// See: https://docs.microsoft.com/en-us/cpp/build/x64-software-conventions?view=vs-2019#register-volatility-and-preservation
#[naked]
#[cfg(target_os="windows")]
unsafe fn switch(old: *mut ThreadContext, new: *const ThreadContext) {
    asm!("
        mov     %rsp, 0x00($0)
        mov     %r15, 0x08($0)
        mov     %r14, 0x10($0)
        mov     %r13, 0x18($0)
        mov     %r12, 0x20($0)
        mov     %rbx, 0x28($0)
        mov     %rbp, 0x30($0)

        mov     0x00($1), %rsp
        mov     0x08($1), %r15
        mov     0x10($1), %r14
        mov     0x18($1), %r13
        mov     0x20($1), %r12
        mov     0x28($1), %rbx
        mov     0x30($1), %rbp
        mov     0x38($1), %rcx
        ret
        "
    : "=*m"(old)
    : "r"(new)
    :
    : "alignstack" 
    );
}


fn main() {
    let mut runtime = Runtime::new();
    runtime.init();
    // we need to simulate a reactor that drives our task forward
    let s = 1;
    runtime.spawn(async move || {
        println!("THREAD 1 STARTING");
        let id = s;
        for i in 0..10 {
            println!("thread: {} counter: {}", id, i);
            yield_thread();
        }
    });
    runtime.spawn(async || {
        println!("THREAD 2 STARTING");
        let id = 2;
        for i in 0..15 {
            println!("thread: {} counter: {}", id, i);
            yield_thread();
        }
    });
    runtime.run();
}


use std::thread;
use std::sync::{Arc, Mutex};
use std::ops::Deref;
use std::future::Future;

use std::task::{RawWaker, RawWakerVTable, Context, Poll, Waker};
use std::sync::atomic::{AtomicBool,Ordering};
use std::pin::Pin;
use std::ops::DerefMut;
use std::mem;

// ===== FUTURE =====

struct MyFuture<'a> {
    resource_ready: &'a AtomicBool,
}

// Normally it would seem that a normal Fn would work here, problem is that Waker needs a "wake"
// function in the vtable, and a Fn only has a "call" fn in the vtable. We need to make our own
// Fn() "trait" that is a waker instead
impl<'a> MyFuture<'a> {
    fn new(resource_ready: &'a AtomicBool) -> Self {
        MyFuture {
           resource_ready,
        }
        // let (data, vtable) = unsafe {mem::transmute::<_,(*const (), *const RawWakerVTable)>(waker)};
        // let vtable: &RawWakerVTable = unsafe{&*vtable};
        // MyFuture {
        //     waker: unsafe {Waker::from_raw(RawWaker::new(data, vtable))},
        // }
    }
}


impl<'a> Future for MyFuture<'a> {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, ctx: &mut Context) -> Poll<()> {
        let s = self.deref_mut();
        //s.waker = ctx.waker().clone();
        if self.resource_ready.load(Ordering::Relaxed) {
           Poll::Pending 
        } else {
            Poll::Ready(())
        }
        // check if task is ready
        
    }
}



// ===== REACTOR =====
// see: https://play.rust-lang.org/?version=nightly&mode=debug&edition=2018&gist=ab1d5f264e2be1e946745e982219fb2e

// instead of running this on our main thread (which we could), lets make this easier to mentally
// parse by running it on a seperate OS thread

struct Reactor {
    interested: Vec<Waker>,
    resourcedata: Arc<Mutex<Vec<ResourceEvent>>>,
    resource_ready: AtomicBool,
}
struct SomeResource {
    counter: u32,
    data: Arc<Mutex<Vec<ResourceEvent>>>,
}

enum ResourceEvent {
    Got(u32),
    Finished,
}

/// Simulates a slow resource 
impl SomeResource {
    fn new(data: Arc<Mutex<Vec<ResourceEvent>>>) -> Self {
        SomeResource {
            counter: 0,
            data,
            resource_ready: AtomicBool::new(false),
        }
    }
    fn start(&mut self) {
        for i in 0..10 {
            thread::sleep(std::time::Duration::from_millis(1000));
             self.send(i);
            }
            self.finish();
        }
        

    fn send(&mut self, data: u32) {
        if let Ok(mut r) = self.data.lock() {
            r.push(ResourceEvent::Got(data));
        }
    }

    fn finish(&mut self) {
        if let Ok(mut r) = self.data.lock() {
            r.push(ResourceEvent::Finished);
        }
    }

}

impl Reactor {
    fn new() -> Self {
        Reactor {
            interested: vec![],
            resourcedata: Arc::new(Mutex::new(vec![])),
        }
    }
    fn run(&self) {
        // just spawn a resource, let's pretend we have no control over it and just forget
        // the handle, but we know when the resource signals it's finished
        self.resource_ready = AtomicBool::new(false);
        let data = self.resourcedata.clone();
        thread::spawn(move || {
            let mut resource = SomeResource::new(data);
            resource.start();
        });


        while !self.check_status() {
            thread::yield_now();
            // if we push this reactor to another thread we could just
            // thread::sleep(std::time::Duration::from_millis(100));
        }

        self.resource_ready = AtomicBool::new(true);
        self.call_wakers();
    }

    fn check_status(&self) -> bool {
        match self.resourcedata.lock() {
            Ok(r) => {
                match r.last() {
                    Some(val) => {
                        match val {
                            ResourceEvent::Finished => true,
                            _ => false,
                        }
                    },
                    None => false,
                }
            },
            _ => false,
        }
}

fn call_wakers(&self) {
    for waker in self.interested {
        waker.wake();
    }
}
}




