//! This test is for the gatt_service derive macro. It will check that all attributes and arguments are able to be processed by the macro

use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use trouble_host::prelude::*;

#[gatt_service(uuid = "7e701cf1-b1df-42a1-bb5f-6a1028c793b0")]
struct CustomService {
    #[characteristic(uuid = "2a37", read, on_read = on_read, write, on_write = on_write, app_managed)]
    short_uuid: u8,
    #[characteristic(uuid = "7e711cf1-b1df-42a1-bb5f-6a1028c793b0", write_without_response, indicate)]
    long_uuid: f32,
    #[characteristic(uuid = "2a38", read, notify)]
    notify: [u8; 8],
    non_characteristic_field: u8,
}

#[tokio::test]
async fn gatt_service_derive() {
    let mut table: AttributeTable<NoopRawMutex, 10> = AttributeTable::new();
    let service = CustomService::new(&mut table);

    // Check all fields of service have been generated
    let _handle = service.handle;
    let _non_characteristic_field = service.non_characteristic_field;

    let _characteristic_short_uuid = service.short_uuid;
    assert!(service.short_uuid_on_read.is_some());
    assert!(service.short_uuid_on_write.is_some());

    let _characteristic_long_uuid = service.long_uuid;
    let _long_uuid_store = service.long_uuid_store;
    assert!(service.long_uuid_on_read.is_none());
    assert!(service.long_uuid_on_write.is_none());

    let _notify = service.notify;
    let _notify_store = service.notify_store;
    assert!(service.notify_on_read.is_none());
    assert!(service.notify_on_write.is_none());
}

fn on_read(_connection: Connection) -> &[u8] {
    static DATA: [u8; 2] = [0; 2];
    &DATA[..]
}

fn on_write(_connection: Connection, _data: &[u8]) {}
