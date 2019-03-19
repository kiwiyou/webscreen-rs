use actix::*;
use actix_web::*;
use config::Config;
use std::io;
use std::sync::{Arc, Weak};

fn ws_index(r: &HttpRequest<WebscreenState>) -> Result<HttpResponse, Error> {
    ws::start(r, ImageSocket)
}

fn main() {
    let mut settings = Config::default();
    settings.merge(config::File::with_name("Settings")).unwrap();
    let sys = System::new("webscreen");
    let provider = ScreenProvider::new(settings.get_int("interval").unwrap() as u64).start();
    server::new(move || {
        let state = WebscreenState {
            provider: provider.clone(),
        };
        App::with_state(state)
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
    .start();
    sys.run();
}

struct WebscreenState {
    provider: Addr<ScreenProvider>,
}

struct UpdateScreen(Weak<[u8]>);
impl Message for UpdateScreen {
    type Result = ();
}

struct ScreenProvider {
    interval: u64,
    screen: x11_screenshot::Screen,
    current: Arc<[u8]>,
    subscribers: Vec<Recipient<UpdateScreen>>,
}

impl ScreenProvider {
    fn new(interval: u64) -> Self {
        Self {
            interval,
            screen: x11_screenshot::Screen::open().unwrap(),
            current: Arc::new([0]),
            subscribers: Vec::new(),
        }
    }
}

impl Actor for ScreenProvider {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        ctx.run_interval(
            std::time::Duration::from_millis(self.interval),
            |act, _ctx| {
                act.update();
            },
        );
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
                    self.subscribers
                        .retain(|subscriber| subscriber.try_send(UpdateScreen(b.clone())).is_ok());
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
    type Context = ws::WebsocketContext<Self, WebscreenState>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.state()
            .provider
            .do_send(SubscribeScreen(ctx.address().recipient()));
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

    fn handle(
        &mut self,
        message: UpdateScreen,
        ctx: &mut ws::WebsocketContext<Self, WebscreenState>,
    ) {
        use ws::*;
        let mine = message.0.upgrade().unwrap();
        ctx.send_text(base64::encode(&mine));
    }
}
