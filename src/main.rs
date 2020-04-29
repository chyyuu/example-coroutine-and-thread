#![feature(asm)]
#![feature(naked_functions)]
use std::ptr;

const DEFAULT_STACK_SIZE: usize = 1024 * 1024 * 2;
const MAX_THREADS: usize = 4;
static mut RUNTIME: usize = 0;

pub struct Runtime {
    threads: Vec<Thread>,
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
}

#[derive(Debug, Default)]
#[repr(C)]
struct ThreadContext {
    x1:  u64,
    x2:  u64,
    x8:  u64,
    x9:  u64,
    x18: u64,
    x19: u64,
    x20: u64,
    x21: u64,
    x22: u64,
    x23: u64,
    x24: u64,
    x25: u64,
    x26: u64,
    x27: u64,
    f8:  u32,
    f9:  u32,
    f18: u32,
    f19: u32,
    f20: u32,
    f21: u32,
    f22: u32,
    f23: u32,
    f24: u32,
    f25: u32,
    f26: u32,
    f27: u32,
    nx1: u64,
}

impl Thread {
    fn new(id: usize) -> Self {
        Thread {
            id,
            stack: vec![0_u8; DEFAULT_STACK_SIZE],
            ctx: ThreadContext::default(),
            state: State::Available,
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
        };

        let mut threads = vec![base_thread];
        let mut available_threads: Vec<Thread> = (1..MAX_THREADS).map(|i| Thread::new(i)).collect();
        threads.append(&mut available_threads);

        Runtime {
            threads,
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

        if self.threads[self.current].state != State::Available {
            self.threads[self.current].state = State::Ready;
        }

        self.threads[pos].state = State::Running;
        let old_pos = self.current;
        self.current = pos;

        unsafe {
            switch(&mut self.threads[old_pos].ctx, &self.threads[pos].ctx);
        }

        self.threads.len() > 0
    }

    pub fn spawn(&mut self, f: fn()) {
        let available = self
            .threads
            .iter_mut()
            .find(|t| t.state == State::Available)
            .expect("no available thread.");

        let size = available.stack.len();
        unsafe {
            let s_ptr = available.stack.as_mut_ptr().offset(size as isize);
            let s_ptr = (s_ptr as usize & !15) as *mut u8;
            //ptr::write(s_ptr.offset(-24) as *mut u64, guard as u64);
            //ptr::write(s_ptr.offset(-32) as *mut u64, f as u64);
            //available.ctx.rsp = s_ptr.offset(-32) as u64;

            //ptr::write(s_ptr.ctx.x1 as *mut u64, guard as u64);
            //ptr::write(s_ptr.ctx.nx1 as *mut u64, f as u64);
            available.ctx.x1 = guard as u64;
            available.ctx.nx1 = f as u64;
            available.ctx.x2 = s_ptr.offset(-32) as u64;
            //this->coroutines[pos]->ctx.x1 = (std::uint64_t)(void*)guard;
            //this->coroutines[pos]->ctx.jump_to = (std::uint64_t)(void*)f;
            //this->coroutines[pos]->ctx.x2 = (std::uint64_t)(void*)(s_ptr - 32);
        }
        available.state = State::Ready;
    }
}

fn guard() {
    unsafe {
        let rt_ptr = RUNTIME as *mut Runtime;
        (*rt_ptr).t_return();
    };
}

pub fn yield_thread() {
    unsafe {
        let rt_ptr = RUNTIME as *mut Runtime;
        (*rt_ptr).t_yield();
    };
}

#[naked]
#[inline(never)]
unsafe fn switch(old: *mut ThreadContext, new: *const ThreadContext) {
    asm!("
        sd x1, 0x00($0)
        sd x2, 0x08($0)
        sd x8, 0x10($0)
        sd x9, 0x18($0)
        sd x18, 0x20($0)
        sd x19, 0x28($0)
        sd x20, 0x30($0)
        sd x21, 0x38($0)
        sd x22, 0x40($0)
        sd x23, 0x48($0)
        sd x24, 0x50($0)
        sd x25, 0x58($0)
        sd x26, 0x60($0)
        sd x27, 0x68($0)
        fsw f8, 0x70($0)
        fsw f9, 0x74($0)
        fsw f18, 0x78($0)
        fsw f19, 0x7c($0)
        fsw f20, 0x80($0)
        fsw f21, 0x84($0)
        fsw f22, 0x88($0)
        fsw f23, 0x8c($0)
        fsw f24, 0x90($0)
        fsw f25, 0x94($0)
        fsw f26, 0x98($0)
        fsw f27, 0x9c($0)
        sd x1, 0xa0($0)

        ld x1, 0x00($1)
        ld x2, 0x08($1)
        ld x8, 0x10($1)
        ld x9, 0x18($1)
        ld x18, 0x20($1)
        ld x19, 0x28($1)
        ld x20, 0x30($1)
        ld x21, 0x38($1)
        ld x22, 0x40($1)
        ld x23, 0x48($1)
        ld x24, 0x50($1)
        ld x25, 0x58($1)
        ld x26, 0x60($1)
        ld x27, 0x68($1)
        flw f8, 0x70($1)
        flw f9, 0x74($1)
        flw f18, 0x78($1)
        flw f19, 0x7c($1)
        flw f20, 0x80($1)
        flw f21, 0x84($1)
        flw f22, 0x88($1)
        flw f23, 0x8c($1)
        flw f24, 0x90($1)
        flw f25, 0x94($1)
        flw f26, 0x98($1)
        flw f27, 0x9c($1)
        ld t0, 0xa0($1)

        jr t0
    "
    :
    :"r"(old), "r"(new)
    :
    : "volatile", "alignstack"
    );
}

fn main() {
    let mut runtime = Runtime::new();
    runtime.init();
    runtime.spawn(|| {
        println!("THREAD 1 STARTING");
        let id = 1;
        for i in 0..10 {
            println!("thread: {} counter: {}", id, i);
            yield_thread();
        }
        println!("THREAD 1 FINISHED");
    });
    runtime.spawn(|| {
        println!("THREAD 2 STARTING");
        let id = 2;
        for i in 0..15 {
            println!("thread: {} counter: {}", id, i);
            yield_thread();
        }
        println!("THREAD 2 FINISHED");
    });
    runtime.run();
}
