//! Discover Bluetooth devices and list them.

use bluer::{Address, DeviceEvent, AdapterEvent, Adapter};
use bluer::{gatt::remote::Characteristic, Device, Result};
use std::time::{Duration, SystemTimeError};
use std::time::{SystemTime, UNIX_EPOCH};

use futures::{pin_mut, Stream, stream::SelectAll, StreamExt};
use std::{collections::HashSet, env};
use std::{thread, time};
use std::ops::Deref;
use myfitnesspal_client;
use tokio::time::sleep;
use tokio::io::AsyncWriteExt;
use tokio::io::AsyncReadExt;
use std::mem;
use std::pin::Pin;

async fn query_device(adapter: &Adapter, addr: Address) -> bluer::Result<()> {
    let device = adapter.device(addr)?;
    println!("    Address type:       {}", device.address_type().await?);
    println!("    Name:               {:?}", device.name().await?);
    println!("    Icon:               {:?}", device.icon().await?);
    println!("    Class:              {:?}", device.class().await?);
    println!("    UUIDs:              {:?}", device.uuids().await?.unwrap_or_default());
    println!("    Paired:             {:?}", device.is_paired().await?);
    println!("    Connected:          {:?}", device.is_connected().await?);
    println!("    Trusted:            {:?}", device.is_trusted().await?);
    println!("    Modalias:           {:?}", device.modalias().await?);
    println!("    RSSI:               {:?}", device.rssi().await?);
    println!("    TX power:           {:?}", device.tx_power().await?);
    println!("    Manufacturer data:  {:?}", device.manufacturer_data().await?);
    println!("    Service data:       {:?}", device.service_data().await?);
    Ok(())
}

async fn query_all_device_properties(adapter: &Adapter, addr: Address) -> bluer::Result<()> {


    //let props = device.all_properties().await?;
    //for prop in props {
        //println!("    {:?}", &prop);
    //}
    Ok(())
}
async fn get_scale_timestamp_bytes() -> Result<[u8;4]>{
    let SCALE_UNIX_TIMESTAMP_OFFSET:u64 = 946702800;
    let current_timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let scale_timestamp:u32 = (current_timestamp-SCALE_UNIX_TIMESTAMP_OFFSET).try_into().unwrap();
    Ok(scale_timestamp.to_le_bytes())
}
async fn exercise_characteristic(char: &Characteristic) -> Result<()> {
    println!("    Characteristic flags: {:?}", char.flags().await?);
    //sleep(Duration::from_secs(1)).await;
    println!("    Starting notification session");
    {
        let notify = char.notify().await?;
        pin_mut!(notify);
        println!("    Stopping notification session");
    }
    Ok(())
}
async fn get_notify_object(char: &Characteristic) -> Result<impl Stream<Item = Vec<u8>>> {
    println!("    Characteristic flags: {:?}", char.flags().await?);
    //sleep(Duration::from_secs(1)).await;
    println!("    Starting notification session");
    {
        let notify = char.notify().await?;
        let pinned_notify = notify;
        println!("    Stopping notification session");
        Ok(pinned_notify)
    }
}
async fn sumChecksum(data: &mut Vec<u8>, offset:usize, length:usize) -> u8{
    let mut checksum:u8 = 0;
    for i in 0..length{
        checksum += data[i+offset];
    }
    return checksum;
}
async fn find_our_characteristic(device: &Device, characteristic_uuid: &bluer::Uuid) -> Result<Option<Characteristic>> {
    let SERVICE_UUID = bluer::Uuid::parse_str("0000ffe000001000800000805f9b34fb").unwrap();
    static SERVICE_UUID2: &'static str = "0000ffe0-0000-1000-8000-00805f9b34fb";
    static CHARACTERISTIC_UUID2: &'static str = "0000ffe1-0000-1000-8000-00805f9b34fb";
    let addr = device.address();
    let uuids = device.uuids().await?.unwrap_or_default();
    println!("Discovered device {} with service UUIDs {:?}", addr, &uuids);
    let md = device.manufacturer_data().await?;
    println!("    Manufacturer data: {:x?}", &md);

    if uuids.contains(&SERVICE_UUID) {
        println!("    Device provides our service!");

        //sleep(Duration::from_secs(2)).await;
        if !device.is_connected().await? {
            println!("    Connecting...");
            let mut retries = 2;
            loop {
                match device.connect().await {
                    Ok(()) => break,
                    Err(err) if retries > 0 => {
                        println!("    Connect error: {}", &err);
                        retries -= 1;
                    }
                    Err(err) => return Err(err),
                }
            }
            println!("    Connected");
        } else {
            println!("    Already connected");
        }

        println!("    Enumerating services...");
        for service in device.services().await? {
            let uuid = service.uuid().await?;
            println!("    Service UUID: {}", &uuid);
            println!("    Service data: {:?}", service.all_properties().await?);
            if uuid == SERVICE_UUID {
                println!("    Found our service!");
                for char in service.characteristics().await? {
                    let uuid = char.uuid().await?;
                    println!("    Characteristic UUID: {}", &uuid);
                    //println!("    Characteristic data: {:?}", char.all_properties().await?);
                    if uuid == *characteristic_uuid {
                        println!("    Found our characteristic!");
                        return Ok(Some(char));
                    }
                }
            }
        }

        println!("    Not found!");
    }

    Ok(None)
}
#[derive(Debug)]
enum MeasurementState{
    unknown,
    measuring,
}
#[derive(Debug)]
struct Measurement {
    state: MeasurementState,
    weight: f32,
    weight_reading_done: bool,
    resistance: [i32; 2],
    resistance_reading_done: bool,
}
fn populate_measurement(bytes: &Vec<u8>) -> Measurement {
    let state_byte = bytes[0];

    let state = match state_byte {
        0x10 => MeasurementState::measuring,
        _ => MeasurementState::unknown,
    };

    let weight_bytes: [u8; 2] = [bytes[3], bytes[4]];
    let weight = (u16::from_be_bytes(weight_bytes) as f32)/100f32;
    let weight_reading_done = bytes[5] != 0;

    let resistance = [
        i16::from_be_bytes([bytes[6], bytes[7]]) as i32,
        i16::from_be_bytes([bytes[8], bytes[9]]) as i32,
    ];
    let resistance_reading_done = bytes[10] != 0;

    Measurement {
        weight,
        weight_reading_done,
        resistance,
        resistance_reading_done,
        state,
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> bluer::Result<()> {
    let test = myfitnesspal_client::add(1,2);
    let with_changes = env::args().any(|arg| arg == "--changes");
    let all_properties = env::args().any(|arg| arg == "--all-properties");
    let filter_addr: HashSet<_> = env::args().filter_map(|arg| arg.parse::<Address>().ok()).collect();

    env_logger::init();
    let session = bluer::Session::new().await?;
    let adapter = session.default_adapter().await?;
    println!("Discovering devices using Bluetooth adapater {}\n", adapter.name());
    adapter.set_powered(true).await?;

    let device_events = adapter.discover_devices().await?;
    pin_mut!(device_events);

    let mut all_change_events = SelectAll::new();
    let scale_address = Address::new([0xA4u8, 0xC1u8, 0x38u8, 0xEDu8, 0xB0u8, 0x0Du8]);
    loop {
        tokio::select! {
            Some(device_event) = device_events.next() => {
                match device_event {
                    AdapterEvent::DeviceAdded(addr) => {
                        if !filter_addr.is_empty() && !filter_addr.contains(&addr) {
                            continue;
                        }

                        println!("Device added: {}", addr);
                        if addr==scale_address {
                            println!("Gotcha bitch");
                            let device = adapter.device(addr)?;
                            if !device.is_connected().await? {
                            let mut retries = 5;
                            loop {
                                match device.connect().await {
                                    Ok(()) => break,
                                    Err(err) if retries > 0 => {
                                        println!("    Connect error: {}", &err);
                                        retries -= 1;
                                    }
                                    Err(err) => return Err(err),
                                }
                            }
                                println!("    Connected");
                            } else {
                                println!("    Already connected");
                            }
                            let CUSTOM1_CHARACTERISTIC_UUID = bluer::Uuid::parse_str("0000ffe100001000800000805f9b34fb").unwrap();
                            let CUSTOM2_CHARACTERISTIC_UUID = bluer::Uuid::parse_str("0000ffe200001000800000805f9b34fb").unwrap();
                            let CUSTOM3_CHARACTERISTIC_UUID = bluer::Uuid::parse_str("0000ffe300001000800000805f9b34fb").unwrap();
                            let CUSTOM4_CHARACTERISTIC_UUID = bluer::Uuid::parse_str("0000ffe400001000800000805f9b34fb").unwrap();
                            let mut weight_notify_obj = None;
                            match find_our_characteristic(&device, &CUSTOM1_CHARACTERISTIC_UUID).await {
                                    Ok(Some(char)) =>  {
                                     weight_notify_obj = Some(get_notify_object(&char).await.unwrap());
                                    //pin_mut!(weight_notify_obj);
                                },
                                Ok(None) => (),
                                Err(err) => {
                                    println!("    Device failed: {}", &err);
                                    let _ = adapter.remove_device(device.address()).await;
                                }
                            }
                            match find_our_characteristic(&device, &CUSTOM2_CHARACTERISTIC_UUID).await {
                                    Ok(Some(char)) => match exercise_characteristic(&char).await {
                                     Ok(()) => {
                                         println!("    Characteristic 2 exercise completed");
                                        }
                                    Err(err) => {
                                     println!("    Characteristic 2 exercise failed: {}", &err);
                                    }
                                },
                                Ok(None) => (),
                                Err(err) => {
                                    println!("    Device failed: {}", &err);
                                    let _ = adapter.remove_device(device.address()).await;
                                }
                            }
                           // write magicnumber 0x130915[WEIGHT_BYTE]10000000[CHECK_SUM] to 0xffe3
                            // 0x01 weight byte = KG. 0x02 weight byte = LB.
                            let weightUnitByte:u8 = 0x01;
                            let mut magicbytes = vec![0x13, 0x09, 0x15, weightUnitByte, 0x10, 0x00, 0x00, 0x00, 0x00];
                            // Set last byte to be checksum
                            let checksum_idx = magicbytes.len()-1;
                            let checksum_range = magicbytes.len() - 1;
                            let checksum = sumChecksum(&mut magicbytes, 0, checksum_range).await;
                            magicbytes[checksum_idx] = checksum;
                            match find_our_characteristic(&device, &CUSTOM3_CHARACTERISTIC_UUID).await {
                                    Ok(Some(char)) => {
                                    println!("    Writing characteristic value {:x?} using function call", &magicbytes);
                                    println!("result: {:?}",char.write(&magicbytes).await?);
                                },
                                Ok(None) => (),
                                Err(err) => {
                                    println!("    Device failed: {}", &err);
                                    let _ = adapter.remove_device(device.address()).await;
                                }
                            }

                            match find_our_characteristic(&device, &CUSTOM4_CHARACTERISTIC_UUID).await {
                                    Ok(Some(char)) => {

                                        match get_scale_timestamp_bytes().await {
                                            Ok(bytes) => {
                                                let timeMagicBytes = [0x02u8, bytes[0],bytes[1], bytes[2],bytes[3]];
                                                println!("    Writing characteristic4 value {:x?} using function call", &timeMagicBytes);
                                                println!("result: {:?}",char.write(&bytes).await?);

                                            }
                                        Err(_) => {println!("Couldn't calculate timestamp");},
                                        }
                                },
                                Ok(None) => (),
                                Err(err) => {
                                    println!("    Device failed: {}", &err);
                                    let _ = adapter.remove_device(device.address()).await;
                                }

                            }
                            let weight_notify_obj_pinned = weight_notify_obj.unwrap();
                            pin_mut!(weight_notify_obj_pinned);
                            for _ in 0..5000u32 {

                            match weight_notify_obj_pinned.next().await {
                                    Some(value) => {
                                    println!("    Weight value: {:x?}", &value);
                                        println!("Decoded weight: {:?}", populate_measurement(&value));
                                }
                                None => {
                                    println!("    Notification session was terminated");
                                }
                            }
        }


                            }
                        let res = if all_properties {
                            query_all_device_properties(&adapter, addr).await
                        } else {
                            query_device(&adapter, addr).await
                        };
                        if let Err(err) = res {
                            println!("    Error: {}", &err);
                        }

                        if with_changes {
                            let device = adapter.device(addr)?;
                            let change_events = device.events().await?.map(move |evt| (addr, evt));
                            all_change_events.push(change_events);
                        }
                    }
                    AdapterEvent::DeviceRemoved(addr) => {
                        println!("Device removed: {}", addr);
                    }
                    _ => (),
                }
                println!();
            }
            Some((addr, DeviceEvent::PropertyChanged(property))) = all_change_events.next() => {
                println!("Device changed: {}", addr);
                println!("    {:?}", property);
            }
            else => break
        }
    }

    Ok(())
}