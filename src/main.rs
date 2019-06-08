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

struct Action {
    exit: bool,
    refresh_led_info: bool,
    refresh_selected: bool,
    refresh_info: bool,
    info: Option<String>,
    selected: i32,
    value_type: Option<ValueType>,
    new_value: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
enum ValueType {
    Current,
    Pwm,
}

impl ValueType {
    fn from_cmd(ch:i32) -> Option<Self> {
        match ch {
            CMD_VALUE_CURRENT => Some(ValueType::Current),
            CMD_VALUE_PWM => Some(ValueType::Pwm),
            _ => None,
        }
    }
}

impl std::fmt::Display for ValueType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ValueType::Current => write!(f, "Current"),
            ValueType::Pwm => write!(f, "PWM"),
        }
    }
}

struct State {
    selected: i32,
    value_type: Option<ValueType>,
    new_value: Option<u32>,
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

const NO_LED: i32 = -1;
const NUM_LEDS: usize = 24;
const GLOBAL_LED: i32 = 24;

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

fn run(conf: &Config, core: &mut Core, client: &Client) {
    output_template();
    let mut state = State {
        selected: NO_LED,
        value_type: None,
        new_value: None
    };
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
            state.value_type = action.value_type;
            output_selected(&state, &last_info);
        }
        if action.refresh_info {
            output_info(&action.info.unwrap());
        }
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
type CharStatus = [char; NUM_LEDS];

fn print_status_chars(arr: [char; NUM_LEDS]) {
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
    printw(" Select operation:  Off <1>  On <2>  PWM <3>  PWMPlus <4>\n");
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
    refresh();
}

fn output_status(info: &Vec<LedInfo>) {
    let mut status: CharStatus = ['.'; NUM_LEDS];
    let mut errors: CharStatus = ['.'; NUM_LEDS];

    info.iter().
        enumerate().
        for_each(|(ii,x)| {
            match x.state.unwrap() {
                LedState::FALSE => status[ii] = '.',
                LedState::TRUE => status[ii] = 'o',
                LedState::PWM => status[ii] = 'p',
                LedState::PWMPLUS => status[ii] = '+',
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
    refresh();
}

fn dashes(num: usize) -> String {
    let mut dashes = String::new();
    (0..num).into_iter().for_each(|_| dashes.push('-'));
    dashes
}

fn output_selected(state: &State, last_info: &Vec<LedInfo>) {
    let led = state.selected;
    assert!(led >= NO_LED && led <= GLOBAL_LED);
    let mut selected = format!("{}", led);
    let val_type = match &state.value_type {
        Some(x) => x.to_string(),
        None => dashes(7),
    };
    let value = match state.value_type {
        Some(x) => {
            let val = get_value(last_info, &x, led);
            match val {
                Some(x) => format!("{}", x),
                None => dashes(3),
            }
        },
        None => dashes(3),
    };
    let status;
    if led == GLOBAL_LED {
        selected = "**".to_string();
        status = dashes(7);
    } else if led == NO_LED {
        selected = dashes(2);
        status = dashes(7);
    } else {
        let l: LedState2 = last_info[led as usize].state.unwrap().into();
        status = l.to_string();
    }
    let new_val = match state.new_value {
        Some(x) => format!("{}", x),
        None => dashes(3),
    };
    mvprintw(
        SELECTED_LINE, 
        0, 
        &format!(
            " Selected: {:>2}  Status: {:<7}  Val Type: {:<7}  Cur Val: {:<3}  New Val: {:<3}", 
            selected, 
            status,
            val_type,
            value,
            new_val,
        )
    );
    mv(CURSOR_LINE, CURSOR_COLUMN);
    refresh();
}

fn output_info(info: &str) {
    mv(INFO_LINE, INFO_COLUMN);
    clrtoeol();
    printw(info);
    mv(CURSOR_LINE, CURSOR_COLUMN);
    refresh();
}

const CMD_ENTER: i32 = 10; // LF
const CMD_ESC: i32 = 27; // ESC
const CMD_MODE_OFF: i32 = 49; // 1
const CMD_MODE_ON: i32 = 50; // 2
const CMD_MODE_PWM: i32 = 51; // 3
const CMD_MODE_PWMPLUS: i32 = 52; // 4
const CMD_MODES: [i32; 4] = [CMD_MODE_OFF, CMD_MODE_ON, CMD_MODE_PWM, CMD_MODE_PWMPLUS];
const CMD_VALUE_CURRENT: i32 = 53; // 5
const CMD_VALUE_PWM: i32 = 54; // 6
const CMD_VALUES_LED: [i32; 2] = [CMD_VALUE_CURRENT, CMD_VALUE_PWM];
const CMD_VALUE_OFFSET: i32 = 55; // 7
const CMD_VALUE_GRPFREQ: i32 = 56; // 8
const CMD_VALUE_GRPPWM: i32 = 57; // 9
const CMD_VALUE_DIMBLNK: i32 = 48; // 0
const CMD_VALUES_GLOBAL: [i32; 4] = [CMD_VALUE_OFFSET, CMD_VALUE_GRPFREQ, CMD_VALUE_GRPPWM, CMD_VALUE_DIMBLNK];
const CMD_UP: i32 = 'A' as i32; // Up arrow is 10, 91, 65.  65 = A
const CMD_DOWN: i32 = 'B' as i32; // Down arrow is 10, 91, 66.  66 = B

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

fn set_led_state(conf: &Config, core: &mut Core, client: &Client, led: i32, state: LedState) -> String {
    let s2: LedState2 = state.into();
    output_info(&format!("Setting LED {} to {}", led, s2));
    let result = core.run(client.set_led_state(conf.bus, conf.addr, led, state));
    match result {
        Ok(_) => format!("Set LED {} to {}", led, s2),
        _ => {
            info!("Failed to set LED {} to {}: {:?}\n", led, s2, result);
            format!("Failed to set LED {} to {}", led, s2)
        },
    }
}    

fn valid_led(led: i32) -> bool {
    led >= 0 && led < NUM_LEDS as i32
}

fn get_value(last_info: &Vec<LedInfo>, ty: &ValueType, led: i32) -> Option<u32> {
    if led >= 0 && led < NUM_LEDS as i32 {
        let led = led as usize;
        if last_info.len() > led {
            Some(match ty {
                ValueType::Current => last_info[led].current.unwrap(),
                ValueType::Pwm => last_info[led].pwm.unwrap(),
            })
        } else {
            None
        }
    } else {
        None // Note global values only writable not readable
    }
}

enum LedState2 {
    OFF,
    ON,
    PWM,
    PWMPLUS
}

impl std::fmt::Display for LedState2 {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            LedState2::OFF => write!(f, "Off"),
            LedState2::ON => write!(f, "On"),
            LedState2::PWM => write!(f, "PWM"),
            LedState2::PWMPLUS => write!(f, "PWMPlus"),
        }
    }
}

impl From<i32> for LedState2 {
    fn from(ch: i32) -> Self {
        match ch {
            CMD_MODE_OFF => LedState2::OFF,
            CMD_MODE_ON => LedState2::ON,
            CMD_MODE_PWM => LedState2::PWM,
            CMD_MODE_PWMPLUS => LedState2::PWMPLUS,
            _ => panic!("Invalid LED state requested")
        }
    }
}

impl From<LedState2> for LedState {
    fn from(state: LedState2) -> Self {
        match state {
            LedState2::OFF => LedState::FALSE,
            LedState2::ON => LedState::TRUE,
            LedState2::PWM => LedState::PWM,
            LedState2::PWMPLUS => LedState::PWMPLUS,
        }
    }
}

impl From<LedState> for LedState2 {
    fn from(state: LedState) -> Self {
        match state {
            LedState::FALSE => LedState2::OFF,
            LedState::TRUE => LedState2::ON,
            LedState::PWM => LedState2::PWM,
            LedState::PWMPLUS => LedState2::PWMPLUS,
        }
    }
}

fn process_input(conf: &Config, core: &mut Core, client: &Client, state: &State, ch: i32) -> Action {
    let mut action = Action {
        exit: false,
        refresh_led_info: false,
        refresh_selected: false,
        refresh_info: false,
        info: None,
        selected: state.selected,
        value_type: state.value_type.clone(),
        new_value: state.new_value,
    };
    if ch == CMD_ENTER {
        output_info("Refreshing LED status ... please wait");
        refresh();
        action.refresh_led_info = true;
        action.refresh_selected = true;
        action.refresh_info = true;
        action.info = Some("Refreshed LED status".to_string());
    } else if CMD_MODES.contains(&ch) {
        let ch: LedState2 = ch.into();
        let ledstate: LedState = ch.into();
        let mut leds = vec![];
        if valid_led(state.selected) {
            leds.push(state.selected);
        } else if state.selected == GLOBAL_LED {
            let mut l = (0..24).collect();
            leds.append(&mut l);
        }
        if !leds.is_empty() {
            for led in leds {
                action.info = Some(set_led_state(conf, core, client, led, ledstate));
            }
            action.refresh_led_info = true;
            action.refresh_selected = true;
            action.refresh_info = true;
        }
    } else if CMD_LEDS.contains(&ch) {
        for (ii, led) in CMD_LEDS.iter().enumerate() {
            if *led == ch {
                action.refresh_selected = true;
                action.selected = ii as i32;
                action.selected -= 1; // 0th index should be -1 - for none
                action.info = Some(if action.selected == GLOBAL_LED {
                    "Selected Global".to_string()
                } else if action.selected == NO_LED {
                    "No LED selected".to_string()
                } else {
                    format!("Selected LED {}", action.selected)
                });
                action.refresh_info = true;
            }
        }
    } else if CMD_VALUES_LED.contains(&ch) {
        action.value_type = ValueType::from_cmd(ch);
        action.info = Some(format!("Selected {} Value", action.value_type.clone().unwrap()));
        action.refresh_selected = true;
        action.refresh_info = true;
    } else if ch == CMD_ESC {
        timeout(0);
        let discard = getch();
        let ch = getch();
        timeout(-1);
        match ch {
            CMD_UP => action.info = Some(format!("Pressed Up")),
            CMD_DOWN => action.info = Some(format!("Pressed Down")),
            -1 => if discard == -1 { // Means ESC was pressed
                action.exit = true;
                action.info = Some("User termination".to_string());
            }
            _ => action.info = Some(format!("Unknown key-press {}, {}", discard, ch)),
        }
        action.refresh_info = true;
    } else {
        action.info = Some(format!("Unknown key-press {}", ch));
        action.refresh_info = true;
    }

    action
}

