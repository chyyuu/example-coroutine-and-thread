#![feature(asm)]
#![feature(naked_functions)]
use std::ptr;

// In our simple example we set most constraints here.
const DEFAULT_STACK_SIZE: usize = 1024 * 1024 * 8;
const MAX_THREADS: usize = 4;
static mut RUNTIME: usize = 0;
static mut STACK_PTR: *mut usize = 0 as *mut usize;

struct Runtime {
    threads: Vec<Thread>,
    current: usize,
}

#[derive(PartialEq, Eq, Debug)]
enum State {
    Parked,
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
#[repr(C)] // not strictly needed but Rust ABI is not guaranteed to be stable
struct ThreadContext {
    rsp: u64,
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    rbx: u64,
    rbp: u64,
}

impl Thread {
    fn new(id: usize) -> Self {
        // We initialize each thread here and allocate the stack. This is not neccesary,
        // we can allocate memory for it later, but it keeps complexity down and lets us focus on more interesting parts
        // to do it here. The important part is that once allocated it MUST NOT move in memory.
        Thread {
            id,
            stack: vec![0_u8; DEFAULT_STACK_SIZE],
            ctx: ThreadContext::default(),
            state: State::Parked,
        }
    }
}

impl Runtime {
    fn new() -> Self {
        // This will be our base thread, which will be initialized in the `running` state
        let base_thread = Thread {
            id: 0,
            stack: vec![0_u8; DEFAULT_STACK_SIZE],
            ctx: ThreadContext::default(),
            state: State::Running,
        };

        // We initialize the rest of our threads.
        let mut threads = vec![base_thread];
        let mut available_threads: Vec<Thread> = (1..MAX_THREADS).map(|i| Thread::new(i)).collect();
        threads.append(&mut available_threads);

        Runtime {
            threads,
            current: 0,
        }
    }

    /// This is cheating a bit, but we need a pointer to our Runtime stored so we can call yield on it even if
    /// we don't have a reference to it.
    fn init(&self) {
        unsafe {
            let r_ptr: *const Runtime = self;
            RUNTIME = r_ptr as usize;
        }
    }

    /// This is where we start running our runtime. If the current thread is not our base thread we set its state to
    /// Parked. It means we're finished with it. Then we yield which will schedule a new thread to be run.
    /// If it is our base thread, we call yield until it returns false (which means that there are no threads scheduled)
    /// and we are done.
    fn run(&mut self) -> ! {
        let current = self.current;
        if current != 0 {
            self.threads[current].state = State::Parked;
            self.t_yield();
        }

        while self.t_yield() {}
        std::process::exit(0);
    }
    
    /// This is the heart of our runtime. Here we go through all threads and see if anyone is in the `Ready` state.
    /// If no thread is `Ready` we're all done. This is an extremely simple sceduler using only a round-robin algorithm.
    /// 
    /// If we find a thread that's ready to be run we change the state of the current thread from `Running` to `Ready`.
    /// Then we call swictch wich will save the current context (the old context) and load the new context
    /// into the CPU which then resumes based on the context it was just passed.
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

        if self.threads[self.current].state != State::Parked {
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

    /// While `yield` is the logically interesting function I think this the technically most interesting. 
    /// 
    /// 
    /// When we spawn a new thread we first check if there are any available threads (threads in `Parked` state). 
    /// If we run out of threads we panic in this scenario but there are several (better) ways to handle that. 
    /// We keep things simple for now.
    /// 
    /// When we find an available thread we get the stack length and a pointer to our u8 bytearray.
    /// 
    /// The next part we have to use some unsafe functions. First we write an address to our `guard` function
    /// that will be called if the function we provide returns. Then we set the address to the function we 
    /// pass inn.
    /// 
    /// Third, we set the value of `rsp` which is the stack pointer to the address of our provided function so we start
    /// executing that first when we are scheuled to run.
    /// 
    /// Lastly we set the state as `Ready` which means we have work to do and is ready to do it.
    fn spawn(&mut self, f: fn()) {
        let available = self
            .threads
            .iter_mut()
            .find(|t| t.state == State::Parked)
            .expect("no available thread.");

        let size = available.stack.len();
        let s_ptr = available.stack.as_mut_ptr();

        unsafe {
            ptr::write(s_ptr.offset((size - 8) as isize) as *mut u64, guard as u64);
            ptr::write(s_ptr.offset((size - 16) as isize) as *mut u64, f as u64);
            available.ctx.rsp = s_ptr.offset((size - 16) as isize) as u64;
        }

        available.state = State::Ready;
    }
}

/// This is our guard function that we place on top of the stack. All this function does is set the 
/// state of our current thread and then `yield` which will then schedule a new thread to be run.
fn guard() -> ! {
    unsafe {
        let rt_ptr = RUNTIME as *mut Runtime;
        let rt = &mut *rt_ptr;
        println!("THREAD {} FINISHED.", rt.threads[rt.current].id);
        rt.run();
    };
    
}

/// We know that Runtime is alive the length of the program and that we only access from one core 
/// (so no datarace). We yield execution of the current thread  by dereferencing a pointer to our 
/// Runtime and then calling `t_yield` 
fn yield_thread() {
    unsafe {
        let rt_ptr = RUNTIME as *mut Runtime;
        (*rt_ptr).t_yield();
    };
}

/// So here is our inline Assembly. As you remember from our first example this is just a bit more elaborate where we first
/// read out the values of all the registers we need and then sets all the register values to the register values we 
/// saved when we suspended exceution on the "new" thread.
/// 
/// This is essentially all we need to do to save and resume execution.
/// 
/// Some details about inline assembly:
/// First ":" after ":" we have our output parameters, this is values we write data to. We use "=*m" since we pass a pointer
/// in and we want to write to the location of the data the pointer points to.
/// Second ":" we have the input parameters which is our "new" context. We only read from this data.
/// Third ":" This our clobber list, this is information to the compiler that these registers can't be used freely
/// Fourth ":" This is options we can pass inn, Rust has 3: "alignstack", "volatile" and "intel"
/// For this to work (partially) on windows we need to use "alignstack" where the compiler adds the neccesary padding to
/// make sure our stack is aligned.
/// 
/// One last important part (it will not work without this) is the #[naked] attribute. Basically this lets us have full
/// control over the stack layout since normal functions has a prologue-and epilogue added by the
/// compiler that will cause trouble for us. We avoid this by marking the funtion as "Naked".

// see: https://github.com/rust-lang/rfcs/blob/master/text/1201-naked-fns.md
#[naked]
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
        ret
        
        "
    : "=*m"(old)
    : "r"(new)
    :
    : "alignstack" // needed to work on windows
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
    });

    runtime.spawn(|| {
        println!("THREAD 2 STARTING");
        let id = 2;
        for i in 0..15 {
            println!("thread: {} counter: {}", id, i);
            yield_thread();
        }
    });
    runtime.run();
}
