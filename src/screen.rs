use actix_web::actix::*;
use std::io;
use std::sync::{Arc, RwLock, Weak};

pub struct UpdateScreen(pub Box<[u8]>);
impl Message for UpdateScreen {
    type Result = ();
}

pub struct ScreenProvider {
    interval: u64,
    capturer: Addr<Capturer>,
    current: Arc<[u8]>,
    subscribers: Arc<RwLock<Vec<Recipient<UpdateScreen>>>>,
}

struct Capturer;

impl Actor for Capturer {
    type Context = Context<Self>;
}

struct RequestCapture;

impl Message for RequestCapture {
    type Result = Result<Vec<u8>, io::Error>;
}

impl Handler<RequestCapture> for Capturer {
    type Result = Result<Vec<u8>, io::Error>;

    fn handle(&mut self, _message: RequestCapture, _ctx: &mut Context<Self>) -> Self::Result {
        let screen = scrap::Display::primary()?;
        let (width, height) = (screen.width(), screen.height());
        let mut device = scrap::Capturer::new(screen)?;
        loop {
            match device.frame() {
                Ok(frame) => {
                    let mut flipped = Vec::with_capacity(width * height * 4);
                    let stride = frame.len() / height;
                    for y in 0..height {
                        for x in 0..width {
                            let i = stride * y + 4 * x;
                            flipped.extend_from_slice(&[frame[i + 2], frame[i + 1], frame[i], 255]);
                        }
                    }
                    let mut cursor = io::Cursor::new(Vec::new());
                    repng::encode(&mut cursor, width as u32, height as u32, &flipped)?;
                    return Ok(cursor.into_inner());
                }
                Err(error) => match error.kind() {
                    io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5))
                    }
                    _ => {
                        return Err(error);
                    }
                },
            }
        }
    }
}

impl ScreenProvider {
    pub fn new(interval: u64) -> Result<Self, io::Error> {
        Ok(Self {
            interval,
            capturer: Arbiter::start(|_| Capturer),
            current: Arc::new([0]),
            subscribers: Arc::new(RwLock::new(Vec::new())),
        })
    }
}

impl Actor for ScreenProvider {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        ctx.run_interval(
            std::time::Duration::from_millis(self.interval),
            |act, ctx| {
                act.update(ctx);
            },
        );
    }
}

impl ScreenProvider {
    fn update(&mut self, _ctx: &mut Context<Self>) {
        use futures::future::Future;
        let read_lock = self.subscribers.read().unwrap();
        if !read_lock.is_empty() {
            let subscriber = self.subscribers.clone();
            Arbiter::spawn(
                self.capturer
                    .send(RequestCapture)
                    .map(move |result| match result {
                        Ok(buffer) => {
                            let b: Box<[u8]> = buffer.into();
                            {
                                let mut write_lock = subscriber.write().unwrap();
                                write_lock.retain(|subscriber| {
                                    subscriber.try_send(UpdateScreen(b.clone())).is_ok()
                                })
                            }
                        }
                        Err(error) => log::error!("Error capturing screen: {}", error),
                    })
                    .map_err(|_| {}),
            );
        }
    }
}

pub struct GetScreen;

impl Message for GetScreen {
    type Result = Result<Weak<[u8]>, io::Error>;
}

impl Handler<GetScreen> for ScreenProvider {
    type Result = Result<Weak<[u8]>, io::Error>;

    fn handle(&mut self, _message: GetScreen, _ctx: &mut Context<Self>) -> Self::Result {
        Ok(Arc::downgrade(&self.current))
    }
}

pub struct SubscribeScreen(pub Recipient<UpdateScreen>);

impl Message for SubscribeScreen {
    type Result = ();
}

impl Handler<SubscribeScreen> for ScreenProvider {
    type Result = ();

    fn handle(&mut self, message: SubscribeScreen, _ctx: &mut Context<Self>) -> Self::Result {
        let mut write_lock = self.subscribers.write().unwrap();
        write_lock.push(message.0);
    }
}
