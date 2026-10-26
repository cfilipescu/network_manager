#![cfg(all(unix, feature = "device", feature = "settings"))]

use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use rusty_network_manager::{DeviceProxy, NetworkManagerProxy, SettingsConnectionProxy};
use zbus::{Guid, OwnedObjectPath, OwnedVariant, Variant, connection::Builder};

type Settings = HashMap<String, HashMap<String, OwnedVariant>>;

const DEVICE_PATH: &str = "/org/freedesktop/NetworkManager/Devices/1";
const SETTINGS_PATH: &str = "/org/freedesktop/NetworkManager/Settings/1";

struct NetworkManager;

#[zbus::interface(name = "org.freedesktop.NetworkManager")]
impl NetworkManager {
    fn get_device_by_ip_iface(&self, iface: &str) -> zbus::fdo::Result<OwnedObjectPath> {
        if iface != "wlan0" {
            return Err(zbus::fdo::Error::InvalidArgs(iface.to_owned()));
        }
        Ok(OwnedObjectPath::try_from(DEVICE_PATH).unwrap())
    }
}

struct Device;

#[zbus::interface(name = "org.freedesktop.NetworkManager.Device")]
impl Device {
    #[zbus(property)]
    fn state(&self) -> u32 {
        100
    }
}

#[derive(Default)]
struct SettingsConnection {
    settings: Settings,
}

#[zbus::interface(name = "org.freedesktop.NetworkManager.Settings.Connection")]
impl SettingsConnection {
    fn update(&mut self, properties: Settings) {
        self.settings = properties;
    }

    fn get_settings(&self) -> Settings {
        self.settings.clone()
    }
}

// Exercise real proxy calls without a system bus, NetworkManager, or network changes.
#[tokio::test]
async fn proxies_use_zbus6_paths_properties_and_variants() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let (server_socket, client_socket) = UnixStream::pair().unwrap();
        let server = Builder::unix_stream(server_socket)
            .server(Guid::generate())
            .p2p()
            .serve_at("/org/freedesktop/NetworkManager", NetworkManager)
            .serve_at(DEVICE_PATH, Device)
            .serve_at(SETTINGS_PATH, SettingsConnection::default())
            .build();
        let client = Builder::unix_stream(client_socket).p2p().build();
        let (_server, client) = tokio::try_join!(server, client).unwrap();

        let nm = NetworkManagerProxy::new(&client).await.unwrap();
        let device_path = nm.get_device_by_ip_iface("wlan0").await.unwrap();
        assert_eq!(device_path.as_str(), DEVICE_PATH);
        let device = DeviceProxy::new_from_path(device_path, &client)
            .await
            .unwrap();
        assert_eq!(device.state().await.unwrap(), 100);

        let settings = SettingsConnectionProxy::new_from_path(
            OwnedObjectPath::try_from(SETTINGS_PATH).unwrap(),
            &client,
        )
        .await
        .unwrap();
        settings
            .update(HashMap::from([(
                "connection",
                HashMap::from([
                    ("id", Variant::from("test-wifi")),
                    ("autoconnect", Variant::from(true)),
                    ("autoconnect-retries", Variant::from(3_i32)),
                ]),
            )]))
            .await
            .unwrap();
        let result = settings.get_settings().await.unwrap();
        let connection = &result["connection"];
        assert_eq!(<&str>::try_from(&connection["id"]).unwrap(), "test-wifi");
        assert!(bool::try_from(&connection["autoconnect"]).unwrap());
        assert_eq!(
            i32::try_from(&connection["autoconnect-retries"]).unwrap(),
            3
        );
    })
    .await
    .expect("D-Bus proxy round trip timed out");
}
