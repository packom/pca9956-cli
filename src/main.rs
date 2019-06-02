use pca9956b_api::{ApiNoContext, ContextWrapperExt};
use tokio_core::{reactor, reactor::Core};
use clap::{App, Arg};
use swagger::{make_context,make_context_ty};
use swagger::{ContextBuilder, EmptyContext, XSpanIdString, Push, AuthData};
use ncurses::{initscr, refresh, getch, endwin, printw};
use log::{debug, warn};
use signal_hook::{register, SIGINT, SIGTERM};

struct Config {
    https: bool,
    host: String,
    port: String,
    bus: i32,
    addr: i32,
}

type ClientContext = make_context_ty!(ContextBuilder, EmptyContext, Option<AuthData>, XSpanIdString);
type Client<'a> = swagger::context::ContextWrapper<'a, pca9956b_api::client::Client<hyper::client::FutureResponse>, ClientContext>;

static QUIT: i32 = 0;

fn main() {
    initscr();
    env_logger::init();
    reg_for_sigs();

    let conf = get_args();
    dump_args(&conf);
    let mut core = reactor::Core::new().unwrap();
    let client = create_client(&conf, &core);
    let client = client.with_context(make_context!(ContextBuilder, EmptyContext, None as Option<AuthData>, XSpanIdString(uuid::Uuid::new_v4().to_string())));

    run(&conf, &mut core, &client);

    endwin();
}

macro_rules! reg_sig {
    ($sig: expr, $fn: tt) => {
        unsafe { register($sig, || $fn()) }
            .and_then(|_| {
                debug!("Registered for {}", stringify!($sig));
                Ok(())
            })
            .or_else(|e| {
                warn!("Failed to register for {} {:?}", stringify!($sig), e);
                Err(e)
            })
            .ok();
    }
}

macro_rules! handle_sig {
    ($sig: expr) => {
        {
            endwin();
            warn!("{} caught - exiting", stringify!($sig));
            std::process::exit(128 + $sig);
        }
    }
}

fn reg_for_sigs() {
    reg_sig!(SIGINT, on_sigint);
    reg_sig!(SIGTERM, on_sigterm);
}

fn on_sigint() {
    handle_sig!(SIGINT);
}

fn on_sigterm() {
    handle_sig!(SIGTERM);
}

fn get_args() -> Config {
    let matches = App::new("pca9956b-cli")
        .arg(Arg::with_name("https")
            .long("https")
            .help("Whether to use HTTPS or not"))
        .arg(Arg::with_name("host")
            .long("host")
            .takes_value(true)
            .default_value("localhost")
            .help("Hostname to contact"))
        .arg(Arg::with_name("port")
            .long("port")
            .takes_value(true)
            .default_value("80")
            .help("Port to contact"))
        .arg(Arg::with_name("bus")
            .long("bus")
            .takes_value(true)
            .default_value("0")
            .help("I2C Bus ID"))
        .arg(Arg::with_name("addr")
            .long("addr")
            .takes_value(true)
            .default_value("0")
            .help("PCA9956B I2C address"))
        .get_matches();

    Config {
        https: matches.is_present("https"),
        host: matches.value_of("host").unwrap().to_string(),
        port: matches.value_of("port").unwrap().to_string(),
        bus: matches.value_of("bus").unwrap().parse::<i32>().unwrap(),
        addr: matches.value_of("addr").unwrap().parse::<i32>().unwrap(),
    }
}

fn dump_args(conf: &Config) {
  printw("Args\n");
  printw(&format!("  https: {}\n", conf.https));
  printw(&format!("  host:  {}\n", conf.host));
  printw(&format!("  port:  {}\n", conf.port));
  printw(&format!("  bus:   {}\n", conf.bus));
  printw(&format!("  addr:  {}\n", conf.addr));
}

fn create_client<'a>(conf: &Config, core: &Core) -> pca9956b_api::client::Client<hyper::client::FutureResponse> {
    let base_url = format!("{}://{}:{}",
                           if conf.https { "https" } else { "http" },
                           conf.host,
                           conf.port);
    if conf.https {
        pca9956b_api::Client::try_new_https(core.handle(), &base_url, "examples/ca.pem")
            .expect("Failed to create HTTPS client")
    } else {
        pca9956b_api::Client::try_new_http(core.handle(), &base_url)
            .expect("Failed to create HTTP client")
    }
}

fn run(conf: &Config, mut core: &mut Core, client: &Client) {
    loop {
        let _info = get_info(&conf, &mut core, &client);
        //output_state(info);
        refresh();
        let ch = get_char();
        process_input(ch);
    }
}

fn get_char() -> i32 {
    let ch = getch();
    printw(&format!("{} {}", 8u8 as char, 8u8 as char));
    ch
}

fn get_info(conf: &Config, core: &mut Core, client: &Client) -> () {
    let result = core.run(client.get_led_info_all(conf.bus, conf.addr));
    printw("get_info ... ");
    printw(&format!("{:?}\n", result));
}

const CMD_Q: i32 = 'q' as i32;
const CMD_X: i32 = 'x' as i32;

fn process_input(ch: i32) {
    match ch {
        CMD_Q => handle_sig!(QUIT),
        CMD_X => handle_sig!(QUIT),
        _ => (),
    }
}

