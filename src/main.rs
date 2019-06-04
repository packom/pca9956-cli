use pca9956b_api::{ApiNoContext, ContextWrapperExt, GetLedInfoAllResponse};
use pca9956b_api::models::{LedInfo, OpError};
use tokio_core::{reactor, reactor::Core};
use clap::{App, Arg};
use swagger::{make_context,make_context_ty};
use swagger::{ContextBuilder, EmptyContext, XSpanIdString, Push, AuthData};
use ncurses::{initscr, refresh, getch, endwin, printw, noecho, cbreak, mvprintw};
use log::{debug, warn, info};
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
static ABORT: i32 = 1;

fn main() {
    initscr();
    noecho();
    cbreak();
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


fn exit(sig: i32, err: String) {
    {
        endwin();
        warn!("Exiting due to {}", err);
        std::process::exit(128 + sig);
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
            .default_value("32")
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
  info!("Arg https: {}\n", conf.https);
  info!("Arg host:  {}\n", conf.host);
  info!("Arg port:  {}\n", conf.port);
  info!("Arg bus:   {}\n", conf.bus);
  info!("Arg addr:  {}\n", conf.addr);
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
        handle_info(get_info(&conf, &mut core, &client));
        refresh();
        process_status(getch());
    }
}

fn get_info(conf: &Config, core: &mut Core, client: &Client) -> GetLedInfoAllResponse {
    let result = core.run(client.get_led_info_all(conf.bus, conf.addr));
    match result {
        Ok(x) => x,
        _ => {
            let err = format!("Failure to get PCA9956B info: {:?}\n", result);
            printw(&err);
            exit(ABORT, err);
            GetLedInfoAllResponse::OperationFailed(OpError{error: Some("API Call Failed".to_string())})
        },
    }
}

fn handle_info(info: GetLedInfoAllResponse) {
    match info {
        GetLedInfoAllResponse::OK(info) => output_status(info),
        _ => {
            let err = format!("Failure to get PCA9956B info: {:?}\n", info);
            printw(&err);
            exit(ABORT, err);
        },
    }
}

const LINE_DASHES: &str = "-------------------------------------------------------------------------------\n";
type CharStatus = [char; 24];

fn print_status_chars(arr: [char; 24]) {
    arr.into_iter().
        enumerate().
        filter(|(ii,x)| {
            printw(&format!("{}", x));
            (ii+1) % 4 == 0
        }).
        for_each(|(_,_)| {printw(" ");});
}

fn output_status(info: Vec<LedInfo>)
{
    let status: CharStatus = ['.'; 24];
    let errors: CharStatus = ['.'; 24];
    // XXX Actually build status and errros

    mvprintw(0, 0, LINE_DASHES);
    printw("                         --- PCA9956B Controller ---\n");
    printw(LINE_DASHES);
    printw(" Select LED:  q-i (0-7)  a-k (8-15)  z-, (16-23)  o (global)   Exit: <Esc>\n");
    printw(" Select operation:  1 Off  2 On  3 PWM  4 PWMPlus\n");
    printw(" Select value:  5 Current  6 PWM  7 Offset  8 GRPFREQ  9 GRPPWM  0 DimBlnk\n");
    printw(" Modify selected value: <up> <down>   Apply selected value: <space>\n");
    printw(LINE_DASHES);
    // Status: .op+ .op+ .op+ .op+ .op+ .op+     Key: . Off  p PWM  + PWMPlus o On    
    // Errors: .sox .... .... .... .... ....     Key: . None o Open s Short   x DNE
    printw(" Status: ");
    print_status_chars(status);
    printw("    Key: . Off  p PWM  + PWMPlus o On\n");
    printw(" Errors: ");
    print_status_chars(errors);
    printw("    Key: . None o Open s Short   x DNE\n");
    printw(LINE_DASHES);
    // Selected: 23  Status: PWMPlus  Value: 255  Applies to: Current  Applied: No  
    printw(&format!(" Selected: {}  Status: {}  Value: {}  Applied to: {}  Applied: {}\n", "", "", "", "", ""));
    printw(LINE_DASHES);
    // ... LED 0: Current 254: Value applied    printw(LINE_DASHES);
    printw(&format!(" ... {}\n", ""));
    printw(LINE_DASHES);

/*
    printw("Led Info ...\n");
    for led in info {
        printw(&format!(
            "  {}: State: {:?} PWM: {}/255 Current {}/255 Error {:?}\n",
            led.index.unwrap(),
            led.state.unwrap(),
            led.pwm.unwrap(),
            led.current.unwrap(),
            led.error.unwrap()
        ));
    }
*/    
}

const CMD_ESC: i32 = 27;

fn process_status(ch: i32) {
    match ch {
        CMD_ESC => exit(QUIT, "User termination".to_string()),
        _ => (),
    }
}

