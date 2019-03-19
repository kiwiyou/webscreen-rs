mod screen;

use actix::*;
use actix_web::*;
use config::Config;
use screen::*;

fn ws_index(r: &HttpRequest<WebscreenState>) -> Result<HttpResponse, Error> {
    ws::start(r, ImageSocket)
}

fn main() {
    let mut settings = Config::default();
    settings.merge(config::File::with_name("Settings")).unwrap();
    let sys = System::new("webscreen");
    let provider = ScreenProvider::new(settings.get_int("interval").unwrap() as u64)
        .unwrap()
        .start();
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
        let mine = message.0;
        ctx.send_text(base64::encode(&mine));
    }
}
