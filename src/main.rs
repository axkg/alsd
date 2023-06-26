// SPDX-FileCopyrightText: © 2023 Alexander König <alex@lisas.de>
// SPDX-License-Identifier: MIT

use std::fs::File;
use std::io::{Error, ErrorKind, Read, Write};
use std::mem::size_of;
use std::path::PathBuf;
use std::process::exit;
use std::slice::from_raw_parts_mut;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::{fs, path::Path, thread, time::Duration};

use mqtt::Message;
use paho_mqtt as mqtt;
use serde_json as json;

const GPIOALS_CANCEL: u8 = 0;
const GPIOALS_ARM: u8 = 1;
const GPIOALS_MEASURE: u8 = 2;
//const GPIOALS_STATISTICS: u8 = 3;

fn mqtt_reconnect(client: &mqtt::Client) -> bool {
    println!("Connection to MQTT broker lost. Reconnecting...");
    loop {
        thread::sleep(Duration::from_millis(3000));
        if client.reconnect().is_ok() {
            println!("Connection to MQTT broker restored.");
            return true;
        }
    }
}

fn find_config() -> Result<String, Error> {
    let config_file_name = "alsd.json";

    if Path::new(config_file_name).exists() {
        return Ok(String::from(config_file_name));
    }

    let user_config_dir = dirs::config_dir();

    if let Some(mut user_config_path) = user_config_dir {
        user_config_path.push(config_file_name);

        let user_config_filename = user_config_path.display().to_string();

        if Path::new(&user_config_filename).exists() {
            return Ok(user_config_filename);
        }
    }

    let mut global_config_path = PathBuf::from("/etc");
    global_config_path.push(config_file_name);

    let global_config_filename = global_config_path.display().to_string();

    if Path::new(&global_config_filename).exists() {
        return Ok(global_config_filename);
    }

    let msg = "configuration file not found";
    Err(Error::new(ErrorKind::NotFound, msg))
}

fn load_config() -> json::Value {
    let config_file_name = find_config().expect("couldn't find alsd configuration");

    println!("Using configuration: \'{config_file_name}\'");

    let json = fs::read_to_string(config_file_name).unwrap_or_else(|_| String::from("{}"));
    let config: json::Value = json::from_str(&json).expect("unable to parse configuration file");

    config
}

fn setup_mqtt_client(config: &json::Value) -> mqtt::Client {
    let mqtt_broker = config["mqtt"]["broker"].as_str().unwrap_or("localhost");

    let mqtt_create_options = mqtt::CreateOptionsBuilder::new()
        .server_uri(mqtt_broker)
        .client_id("alsd")
        .persistence(None)
        .finalize();

    let mqtt_client =
        mqtt::Client::new(mqtt_create_options).expect("failed to instantiate MQTT client");
    let mqtt_connect_options = mqtt::ConnectOptionsBuilder::new()
        .keep_alive_interval(Duration::from_millis(30000))
        .clean_session(false)
        .finalize();

    mqtt_client
        .connect(mqtt_connect_options)
        .expect("failed to connect to MQTT broker");

    mqtt_client
}

#[derive(Debug, Default, Copy, Clone)]
#[repr(C)]
struct GpioAlsMeasurement {
    timestamp: u64,
    value: u64,
}

fn send_command(is_running: &AtomicBool, mut device: &File, command: u8, delay: u64) -> bool {
    device
        .write_all(&[command])
        .expect("failed to send command to character device");
    thread::sleep(Duration::from_millis(delay));

    is_running.load(Ordering::Relaxed)
}

fn main() {
    // read configuration
    let config = load_config();

    // create MQTT client and connect to broker
    let mqtt_client = setup_mqtt_client(&config);

    // flag to signal shutdown
    let is_running = Arc::new(AtomicBool::new(true));

    // open the character device to send commands in a dedicated thread
    let writer_device = File::options()
        .read(true)
        .write(true)
        .open(config["device"].as_str().unwrap_or("/dev/gpioals_device"))
        .expect("failed to open character device");

    // clone the device to read measurements in main thread
    let mut reader_device = writer_device
        .try_clone()
        .expect("failed to clone device for read access");

    // handle termination
    let ctrlc_handler_client = mqtt_client.clone();

    ctrlc::set_handler(move || {
        eprintln!("shutting down on termination signal");
        ctrlc_handler_client.stop_consuming();
        ctrlc_handler_client.disconnect(None).unwrap();
        // exit the hard way, signalling will not work in this case as the read() might be stuck forever
        exit(0);
    })
    .expect("failed to setup signal handler");

    // allow configured threshold for measurement to arrive
    let rate = config["rate"].as_u64().unwrap_or(14000);

    // flag to terminate read and write loops cooperatively
    let writer_is_running = Arc::clone(&is_running);

    // writer thread
    let thread_handle = thread::spawn(move || {
        while writer_is_running.load(Ordering::Relaxed) {
            if !send_command(&writer_is_running, &writer_device, GPIOALS_CANCEL, 500) {
                break;
            }
            if !send_command(&writer_is_running, &writer_device, GPIOALS_ARM, 500) {
                break;
            }
            if !send_command(&writer_is_running, &writer_device, GPIOALS_MEASURE, rate) {
                break;
            }
        }
    });

    // read configured mqtt topic for measurements
    let mqtt_topic = config["mqtt"]["topic"].as_str().unwrap_or("alsd");

    // loop to read measurements and send via MQTT, break on MQTT error
    while is_running.load(Ordering::Relaxed) {
        if !mqtt_client.is_connected() && !mqtt_reconnect(&mqtt_client) {
            is_running.store(false, Ordering::Relaxed);
            break;
        }

        let mut measurement = GpioAlsMeasurement {
            timestamp: 0,
            value: 0,
        };

        unsafe {
            let buffer = from_raw_parts_mut(
                &mut measurement as *mut GpioAlsMeasurement as *mut u8,
                size_of::<GpioAlsMeasurement>(),
            );

            if reader_device.read_exact(buffer).is_ok() {
                let value = measurement.value;
                let message = Message::new(mqtt_topic, value.to_string(), 1);
                mqtt_client
                    .publish(message)
                    .expect("failed to publish measurement");
            } else {
                thread::sleep(Duration::from_millis(500));
            }
        }
    }

    if mqtt_client.is_connected() {
        mqtt_client.disconnect(None).unwrap();
    }

    thread_handle.join().unwrap();
}
