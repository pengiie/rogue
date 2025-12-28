pub fn oneshot<T>() -> (Sender<T>, Reciever<T>) {
    unimplemented!()
}

pub struct Sender<T> {
    channel: Channel<T>,
}

pub struct Reciever<T> {
    channel: Channel<T>,
}

struct Channel<T> {
    buffer: Box<T>,
}
