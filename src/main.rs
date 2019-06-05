use pca9956b_api::{ApiNoContext, ContextWrapperExt, GetLedInfoAllResponse};
use pca9956b_api::models::{LedInfo, OpError, LedState, LedError};
use tokio_core::{reactor, reactor::Core};
use clap::{App, Arg};
use swagger::{make_context,make_context_ty};
use swagger::{ContextBuilder, EmptyContext, XSpanIdString, Push, AuthData};
use ncurses::{initscr, refresh, getch, endwin, printw, noecho, cbreak, mvprintw, mv, clrtoeol, timeout};
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

const START_LINE: i32 = 0;    
const STATUS_LINE: i32 = 9;    
const ERRORS_LINE: i32 = 10;    
const SELECTED_LINE: i32 = 12;    
const INFO_LINE: i32 = 14;    
const INFO_COLUMN: i32 = 5;    
const CURSOR_LINE: i32 = 14;    
const CURSOR_COLUMN: i32 = 78;    

fn main() {
    initscr();
    noecho();
    cbreak();
    timeout(-1);
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


fn exit(sig: i32, err: &str) {
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

struct Action {
    exit: bool,
    refresh_led_info: bool,
    refresh_selected: bool,
    refresh_info: bool,
    info: Option<String>,
    selected: i32,
}

struct State {
    selected: i32,
}

fn run(conf: &Config, core: &mut Core, client: &Client) {
    output_template();
    let mut state = State { selected: -1, };
    let mut last_info: Vec<LedInfo> = vec![];
    let mut action = process_input(conf, core, client, &state, CMD_ENTER); // Reads LED status
    loop {
        if action.exit {
            exit(QUIT, &action.info.clone().unwrap());
        }
        if action.refresh_led_info {
            handle_info(get_info(conf, core, client), &mut last_info);
            output_status(&last_info);
        }
        if action.refresh_selected {
            state.selected = action.selected;
            output_selected(state.selected, &last_info);
        }
        if action.refresh_info {
            output_info(&action.info.unwrap());
        }
        mv(CURSOR_LINE, CURSOR_COLUMN); // End of info line
        refresh();
        action = process_input(conf, core, client, &state, getch());
    }
}

fn get_info(conf: &Config, core: &mut Core, client: &Client) -> GetLedInfoAllResponse {
    let result = core.run(client.get_led_info_all(conf.bus, conf.addr));
    match result {
        Ok(x) => x,
        _ => {
            let err = format!("Failure to get PCA9956B info: {:?}\n", result);
            printw(&err);
            exit(ABORT, &err);
            GetLedInfoAllResponse::OperationFailed(OpError{error: Some("API Call Failed".to_string())})
        },
    }
}

fn handle_info(info: GetLedInfoAllResponse, last_info: &mut Vec<LedInfo>) {
    match info {
        GetLedInfoAllResponse::OK(info) => {
            last_info.clear();
            last_info.append(&mut info.clone());
        },
        _ => {
            let err = format!("Failure to get PCA9956B info: {:?}\n", info);
            printw(&err);
            exit(ABORT, &err);
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

fn output_template() {
    mvprintw(START_LINE, 0, LINE_DASHES);
    printw("                         --- PCA9956B Controller ---\n");
    printw(LINE_DASHES);
    printw(" Select LED:  0-7 <q-i>  8-15 <a-k>  16-23 <z-,>  o (global)  p (none)\n");
    printw(" Select operation:  1 Off  2 On  3 PWM  4 PWMPlus\n");
    printw(" Select value:  5 Current  6 PWM  7 Offset  8 GRPFREQ  9 GRPPWM  0 DimBlnk\n");
    printw(" Modify selected value: <up> <down>   Apply selected value: <space>\n");
    printw(" Exit: <Esc>  Refresh All: <Enter>\n");
    printw(LINE_DASHES);
    // Status: .op+ .op+ .op+ .op+ .op+ .op+     Key: . Off  p PWM  + PWMPlus o On    
    // Errors: .sox .... .... .... .... ....     Key: . None o Open s Short   x DNE
    printw(" Status:                                   Key: . Off  p PWM  + PWMPlus o On\n");
    printw(" Errors:                                   Key: . None o Open s Short   x DNE\n");
    printw(LINE_DASHES);
    // Selected: 23  Status: PWMPlus  Value: 255  Applies to: Current  Applied: No  
    printw("\n");
    printw(LINE_DASHES);
    // ... LED 0: Current 254: Value applied    printw(LINE_DASHES);
    printw(&format!(" ... \n"));
    printw(LINE_DASHES);
    mv(CURSOR_LINE, CURSOR_COLUMN); // End of info line
}

fn output_status(info: &Vec<LedInfo>) {
    let mut status: CharStatus = ['.'; 24];
    let mut errors: CharStatus = ['.'; 24];

    info.iter().
        enumerate().
        for_each(|(ii,x)| {
            match x.state.unwrap() {
                LedState::FALSE => status[ii] = '.',
                LedState::TRUE => status[ii] = 'o',
                LedState::PWM => status[ii] = 'p',
                LedState::PWMPLUS => status[ii] = 'o',
            };
            match x.error.unwrap() {
                LedError::NONE => errors[ii] = '.',
                LedError::SHORT => errors[ii] = 's',
                LedError::OPEN => errors[ii] = 'o',
                LedError::DNE => errors[ii] = 'x',
            };
        });

    // Status: .op+ .op+ .op+ .op+ .op+ .op+     Key: . Off  p PWM  + PWMPlus o On    
    // Errors: .sox .... .... .... .... ....     Key: . None o Open s Short   x DNE
    mvprintw(STATUS_LINE, 0, " Status: ");
    print_status_chars(status);
    printw("    Key: . Off  p PWM  + PWMPlus o On");
    mvprintw(ERRORS_LINE, 0, " Errors: ");
    print_status_chars(errors);
    printw("    Key: . None o Open s Short   x DNE");
    mv(CURSOR_LINE, CURSOR_COLUMN);
}

fn output_selected(led: i32, last_info: &Vec<LedInfo>) {
    assert!(led >= -1 && led <= 24);
    let mut selected = format!("{}", led);
    let status;
    if led == 24 {
        selected = "**".to_string();
        status = "-------";
    } else if led == -1 {
        selected = "--".to_string();
        status = "-------";
    } else {
        status = match last_info[led as usize].state.unwrap() {
            LedState::FALSE => "Off",
            LedState::TRUE => "On",
            LedState::PWM => "PWM",
            LedState::PWMPLUS => "PWMPlus",
        }
    }
    mvprintw(
        SELECTED_LINE, 
        0, 
        &format!(
            " Selected: {:>2}  Status: {:<7}  Value:      Applies to:          Applied:     ", 
            selected, 
            status
        )
    );
    mv(CURSOR_LINE, CURSOR_COLUMN);
}

fn output_info(info: &str) {
    mv(INFO_LINE, INFO_COLUMN);
    clrtoeol();
    printw(info);
    mv(CURSOR_LINE, CURSOR_COLUMN);
}

const CMD_ENTER: i32 = 10;
const CMD_ESC: i32 = 27;
const CMD_LEDS: [i32; 26] = [
    'p' as i32, // -1 = None
    'q' as i32, // LED 0
    'w' as i32, // LED 1
    'e' as i32, 
    'r' as i32, 
    't' as i32, 
    'y' as i32, 
    'u' as i32, 
    'i' as i32, 
    'a' as i32, 
    's' as i32, 
    'd' as i32, 
    'f' as i32, 
    'g' as i32, 
    'h' as i32, 
    'j' as i32, 
    'k' as i32, 
    'z' as i32, 
    'x' as i32, 
    'c' as i32, 
    'v' as i32, 
    'b' as i32, 
    'n' as i32, 
    'm' as i32, 
    ',' as i32, // LED 23
    'o' as i32, // Global
];

fn process_input(_conf: &Config, mut _core: &mut Core, _client: &Client, state: &State, ch: i32) -> Action {
    let mut action = Action{
        exit: false,
        refresh_led_info: false,
        refresh_selected: false,
        refresh_info: false,
        info: None,
        selected: -1,
    };
    match ch {
        CMD_ESC => {
            action.exit = true;
            action.info = Some("User termination".to_string());
        },
        CMD_ENTER => {
            output_info("Refreshing LED status ... please wait");
            refresh();
            action.refresh_led_info = true;
            action.refresh_selected = true;
            action.selected = state.selected;
            action.refresh_info = true;
            action.info = Some("Refreshed LED status".to_string());
        },
        _ => (),
    };
    for (ii, led) in CMD_LEDS.iter().enumerate() {
        if *led == ch {
            action.refresh_selected = true;
            action.selected = ii as i32;
            action.selected -= 1; // 0th index should be -1 - for none
            if action.selected == 24 {
                action.info = Some("Selected Global".to_string());
            } else if action.selected == -1 {
                action.info = Some("No LED selected".to_string());
            } else {
                action.info = Some(format!("Selected LED {}", action.selected));
            };
            action.refresh_info = true;
        }
    }
    action
}

