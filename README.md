# Green Threads Example

This repo was made to accompany article and gitbook.

Gitbook: [https://cfsamson.gitbook.io/green-threads-explained-in-200-lines-of-rust/](https://cfsamson.gitbook.io/green-threads-explained-in-200-lines-of-rust/)

## Branches
There are a few interesting branches:
1. master - this will be the 200 lines of code in the book
2. commented - this is the same 200 lines but extensively commented along the way
3. windows - this is an implementation with a proper context switch on Windows, also a copy of the code in the book
4. trait_objects - this is an implementation where we can take trait objects like `Fn()`, `FnMut()` and `FnOnce()` instead of just function pointers, this is way more useful but currently lags a bit behind the improvements in the first three branches
5. futures - I'm collecting data and playing around to tie this in to Rusts Futures and async story - see below

## Futures
The end goal was (and still is) for me to use this as a basis to investigate and implement a simple example of the Executor-Reactor pattern using
Futures 3.0 and Rusts async/await syntax.

The main idea is that Reading these two books will give most people a pretty deep understanding of async code and how Futures work once finished and
thereby bride a gap between the documentation that is certainly going to come from the Rust docs team and in libraries like `Tokio` and use an example driven
approact to learning the basics pretty much from the ground up. I will not be focusing too much on how exactly `tokio`, `romio` or `mio` works. Since they are good
implementations that carry a large amout of compexity in themselves, this will be a bad implementation, but a working example focusing on what's happening
under the hood.

The threading implementation used in the first book will probably be changed slightly to serve as an `exeutor` and instead of just spawning
`fn()` or `trait objects` we spawen futures.