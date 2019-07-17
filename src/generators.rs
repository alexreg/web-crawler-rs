use std::ops::{Generator, GeneratorState};
use std::pin::Pin;

pub struct GenIter<G: Generator<Yield = I, Return = R> + Unpin, I, R>(pub G);

impl<G: Generator<Yield = I, Return = R> + Unpin, I, R> Iterator for GenIter<G, I, R> {
    type Item = I;

    fn next(&mut self) -> Option<Self::Item> {
        match Pin::new(&mut self.0).resume() {
            GeneratorState::Yielded(item) => Some(item),
            GeneratorState::Complete(_) => None,
        }
    }
}

pub macro gen_iter {
    ($($body:tt)*) => {
        GenIter(|| {
            $($body)*
        })
    }
}
