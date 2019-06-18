#![feature(asm)]
#![feature(naked_functions)]
#![feature(async_await)]
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
    let mut executor = Runtime::new();
    executor.init();
    let db = DbReactor::new();
    let act_users_fut = db.query("SELECT SUM(t1.is_active) FROM users t1 GROUP BY t1.is_active");
    let tot_users_fut = db.query("SELECT COUNT(t1.id) FROM users t1 GROUP BY t1.id");
    // we need to simulate a reactor that drives our task forward
    let s = 1;
    let act_users = executor.scedule(async || {
        let result  = act_users_fut.await?;
        println!("Active users: {}", result);
        result
    });
    let tot_users = executor.scedule(async || {
        let result = tot_users_fut.await?;
        println!("Total users: {}", result);
        result
    });

    executor.run();
    println!("We have {} % active users", (act_users as f64 / tot_users as f64) * 100);
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


// ===== GENERAL STRUCTURE =====
// Executor hands out a clone of a waker that is passed to the reactor (via one or more futures)
// The waker is cloned from the Context that the future gets when polled by an executor
// The reaactor has the responsibility to check the status of the blocking operation, returning a Poll to
// the future that passes it on (?) to the executor. Once a Poll::Pending is recieved the future is not polled again until:
//
// The reactor calls Waker.wake() which signals to the executor that the future is ready to be polled again **presumably**
// returning a value.
//
// Questions that must be answered:
// - How does the wake() method communicate with the exetutor? Do we need two ques, one with `incoming` futures that are ready
// to be polled, and one queue with `parked/sleeping` futures that are waiting to we woken? Wake then just shifts a `task` from
// `sleeping` to `incoming`?
//
//- How do we releate this to Pin<Self> and self referential structs in a way that's easy to understand?
//
// - Should we make two futures and chain them or should we be satisfied with one for a good enough example? Maybe chain on one operation
// just so we se som "user" input in the code
//
// - How will we run the Reactor? The IO operation is already "faked" out in the suggestion below, but in this 1 thread scenario
// should the reactor be ran on the same thread as the executor? Will that be confusing/hard to implement? The only way this will work
// is if we have a sceduler running both the reactor and the executor, or run the executor as a part of the reactor - that will
// be very confusing since the whole point of the Reactor-Executor pattern is to allow for them to be seperated.

// References:
// https://levelup.gitconnected.com/explained-how-does-async-work-in-rust-c406f411b2e2
// https://tokio.rs/docs/internals/runtime-model/

// ===== EXECUTOR =====
// Our main example above will be the executor, instead of running functions we will pass it futures that it will run, and 
// just change the `call` method to work on futures inestad of `Fn()` traits. Needs to be able to `sleep` until woken

// 1. An executor is more or less like one of our threads.
// 2. A task implements Future
// 3. A task is spawned on our thread, and typically stored on the heap Box<Task>, Arc<Task>
// 4. Executor then calls `poll_future_notify` which ensures that the task context is set to the
// **thread local variable** so **task::current()** is able to read it. NB!NB! fut 3.0 already implements this.
// 5. It also passes in a notify handle (i.e. the Waker). All of this is provided by the task::current
// and thats how it is linked to the executor.
// 6. when notify is called it's like this. Notify::notify(thread_id/task_id) => Waker::wake() -> aredy contains this info?
// 7. The waker.wake() method is responsible for performing the scheduling logic??

// On 7. The waker.wake() could be like our `yield` function in that it `yields` the current thread,
// sets the waker.wake(thread_id) -> thread_id to `Ready` and then passes on to the scheduler (round-robin)

// ----- FROM TOKIO DOCS -----
// reference: https://tokio.rs/docs/internals/runtime-model/
// One strategy for implementing an executor is to store each task in a Box and to use a linked list 
// to track tasks that are scheduled for execution. When Notify::notify is called, then the task 
// associated with the identifier is pushed at the end of the scheduled linked list. When the executor 
// runs, it pops from the front of the linked list and executes the task as described above.

// Note that this section does not describe how the executor is run. The details of this are left to 
// the executor implementation. One option is for the executor to spawn one or more threads and 
// dedicate these threads to draining the scheduled linked list. Another is to provide a 
// MyExecutor::run function that blocks the current thread and drains the scheduled linked list.

// We'll stay away from linked list, but use a VecDeque instead, or just a Vec to keep it very simple
// We'll do the first implementation, using our thread implementation to drain the scheduled tasks
// since we already have many of the missing pieces in place.

// ===== FUTURE =====

struct MyFuture<'a> {
    resource_ready: &'a AtomicBool,
}


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

struct DbReactor {
    interested: Vec<Waker>,
    resourcehandles: Vec<Arc<Mutex<ResourceHandle>>>,
}

impl DbReactor {
    pub fn new() -> Self {    
        DbReactor {
            interested: vec![],
            resourcehandles: vec![],
        }
    }
    fn query(&self, qry: &str) {
        // just spawn a resource, let's pretend we have no control over it and just forget
        // the thread handle, in a real world we would not get a handle to some external resource
        // but we will get a signal that it's finished

        // Instead we create our own handle. We give a fixed Id now
        let handle = ResourceHandle::new(self.resourcehandles.len());
        self.resourcehandles.push(Arc::new(Mutex::new(handle)));
        let handle = self.resourcehandles.last().unwrap().clone();
        thread::spawn(move || {
            let mut resource = SomeDatabase::new(handle);
            resource.query(qry.to_string());
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

struct ResourceHandle {
    id: usize,
    finished: bool,
}

impl ResourceHandle {
    fn new(id: usize) -> Self {
        ResourceHandle {
            id,
            finished: false,
        }
    }
}


struct SomeDatabase {
    counter: u32,
    handle: Arc<Mutex<ResourceHandle>>,
}

enum ResourceEvent {
    Got(u32),
    Finished,
}

/// Simulates a database, this is really not any code we need - but we just want to fake a
/// database.
impl SomeDatabase {
    fn new(handle: Arc<Mutex<ResourceHandle>>) -> Self {
        SomeDatabase {
            counter: 0,
            handle,
        }
    }
    fn query(&mut self, qry: String) {
        // just pretend we actually issue a real query
        let result = if qry.contains("SUM") {
            5
        } else {
            10
        };
        thread::sleep(std::time::Duration::from_millis(1000));
        self.send(i);
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

