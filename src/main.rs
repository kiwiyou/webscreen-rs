#![feature(drain_filter)]
use actix::*;
use actix_web::*;
use config::Config;
use std::io;
use std::sync::{Arc, Weak};

fn ws_index(r: &HttpRequest) -> Result<HttpResponse, Error> {
    ws::start(r, ImageSocket)
}

fn main() {
    let mut settings = Config::default();
    settings.merge(config::File::with_name("Settings")).unwrap();
    server::new(|| {
        App::new()
            .resource("/ws/", |r| r.method(http::Method::GET).f(ws_index))
            .resource("/", |r| {
                r.method(http::Method::GET).f(|_| {
                    HttpResponse::Found()
                        .header(http::header::LOCATION, "/index.html")
                        .finish()
                })
            })
            .handler("/", fs::StaticFiles::new("static").unwrap())
            .finish()
    })
    .bind(settings.get_str("bind").unwrap())
    .unwrap()
    .run();
}

struct UpdateScreen(Weak<[u8]>);
impl Message for UpdateScreen {
    type Result = ();
}

struct ScreenProvider {
    screen: x11_screenshot::Screen,
    current: Arc<[u8]>,
    subscribers: Vec<Recipient<UpdateScreen>>,
}

impl Default for ScreenProvider {
    fn default() -> Self {
        Self {
            screen: x11_screenshot::Screen::open().unwrap(),
            current: Arc::new([0]),
            subscribers: Vec::new(),
        }
    }
}

impl Supervised for ScreenProvider {}

impl SystemService for ScreenProvider {}

impl Actor for ScreenProvider {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        // fixed to 100ms per refresh
        ctx.run_interval(std::time::Duration::from_millis(100), |act, _ctx| {
            act.update();
        });
    }
}

impl ScreenProvider {
    fn update(&mut self) {
        if !self.subscribers.is_empty() {
            match self.screen.capture() {
                Some(new) => {
                    {
                        let (width, height) = (new.width(), new.height());
                        let raw = new.into_raw();
                        let mut input = Vec::new();
                        let mut encoder = image::jpeg::JPEGEncoder::new(&mut input);
                        encoder
                            .encode(&raw, width, height, image::ColorType::RGB(8))
                            .unwrap();
                        self.current = input.into();
                    }
                    let b = Arc::downgrade(&self.current);
                    self.subscribers.drain_filter(|subscriber| {
                        subscriber.try_send(UpdateScreen(b.clone())).is_err()
                    });
                }
                None => {}
            }
        }
    }
}

struct GetScreen;

impl Message for GetScreen {
    type Result = Result<Weak<[u8]>, io::Error>;
}

impl Handler<GetScreen> for ScreenProvider {
    type Result = Result<Weak<[u8]>, io::Error>;

    fn handle(&mut self, _message: GetScreen, _ctx: &mut Context<Self>) -> Self::Result {
        Ok(Arc::downgrade(&self.current))
    }
}

struct SubscribeScreen(Recipient<UpdateScreen>);

impl Message for SubscribeScreen {
    type Result = ();
}

impl Handler<SubscribeScreen> for ScreenProvider {
    type Result = ();

    #[allow(unused_must_use)]
    fn handle(&mut self, message: SubscribeScreen, _ctx: &mut Context<Self>) -> Self::Result {
        message
            .0
            .do_send(UpdateScreen(Arc::downgrade(&self.current)));
        self.subscribers.push(message.0);
    }
}

struct ImageSocket;

impl Actor for ImageSocket {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let provider: Addr<ScreenProvider> = System::current().registry().get();
        provider.do_send(SubscribeScreen(ctx.address().recipient()));
    }
}

impl StreamHandler<ws::Message, ws::ProtocolError> for ImageSocket {
    fn handle(&mut self, message: ws::Message, ctx: &mut Self::Context) {
        match message {
            ws::Message::Ping(message) => ctx.pong(&message),
            ws::Message::Close(_) => ctx.stop(),
            _ => (),
        }
    }
}

impl Handler<UpdateScreen> for ImageSocket {
    type Result = ();

    fn handle(&mut self, message: UpdateScreen, ctx: &mut ws::WebsocketContext<Self>) {
        use ws::*;
        let mine = message.0.upgrade().unwrap();
        ctx.send_text(base64::encode(&mine));
    }
}
