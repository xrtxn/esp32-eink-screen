use embassy_embedded_hal::adapter::BlockingAsync;
use esp_storage::FlashStorage;

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct NvsConfig {
    pub ssid: heapless::String<32>,
    pub password: heapless::String<32>,
    pub caldav: Caldav,
}

impl sequential_storage::map::PostcardValue<'_> for NvsConfig {}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct Caldav {
    pub url: heapless::String<128>,
    pub username: heapless::String<32>,
    pub password: heapless::String<32>,
}

const NVS_STORAGE_START: u32 = 0x9000;
const NVS_STORAGE_SIZE: u32 = 0x6000;
const NVS_RANGE: core::ops::Range<u32> = NVS_STORAGE_START..NVS_STORAGE_START + NVS_STORAGE_SIZE;

const CONFIG_KEY: u8 = 1;

pub(crate) async fn read_config(flashstg: FlashStorage<'_>) {
    let mut data_buffer = [0u8; 256];

    let async_flash = BlockingAsync::new(flashstg);

    let mut l = sequential_storage::map::MapStorage::<u8, _, _>::new(
        async_flash,
        const { sequential_storage::map::MapConfig::new(NVS_RANGE) },
        sequential_storage::cache::NoCache::new(),
    );

    let r = l
        .fetch_item::<NvsConfig>(&mut data_buffer, &CONFIG_KEY)
        .await;

    log::info!("Read config: {:?}", r);
}

pub(crate) async fn write_config(flash_stg: FlashStorage<'_>, config: NvsConfig) {
    let async_flash = BlockingAsync::new(flash_stg);

    let mut data_buffer = [0u8; 256];

    let mut serialized_buf = [0u8; 128];
    let _ = postcard::to_slice(&config, &mut serialized_buf).expect("Failed to serialize config");

    let mut l = sequential_storage::map::MapStorage::<u8, _, _>::new(
        async_flash,
        const { sequential_storage::map::MapConfig::new(NVS_RANGE) },
        sequential_storage::cache::NoCache::new(),
    );

    l.store_item(&mut data_buffer, &CONFIG_KEY, &config)
        .await
        .unwrap();

    log::info!("Config written to flash");
}
