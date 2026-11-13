// L7 double-delay: take logind's delay while one is held. Only `NoDelay`
// has `take`; a held delay is a `DelayFd`.
use impl_typestate::lock::{DelayFd, NoDelay};
use spec::lock::Effect;

fn take_again(fd: DelayFd, out: &mut Vec<Effect>) -> DelayFd {
    NoDelay::take(fd, out)
}

fn main() {}
