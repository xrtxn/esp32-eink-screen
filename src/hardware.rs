use core::cell::RefCell;

use critical_section::Mutex;
use esp_hal::{
    gpio::{AnyPin, Input},
    handler, ram,
    rtc_cntl::sleep::{Ext0WakeupSource, TimerWakeupSource, WakeupLevel},
    system::{wakeup_cause, SleepSource},
};

use crate::{BootType, BOOT_TYPES};

const SLEEP_DURATION: u64 = 300;

pub static BUTTON: Mutex<RefCell<Option<Input<'static>>>> = Mutex::new(RefCell::new(None));

pub(crate) fn go_to_deep_sleep(rtc: &mut esp_hal::rtc_cntl::Rtc<'_>) -> ! {
    let sleep_time = core::time::Duration::from_secs(SLEEP_DURATION);
    let timer_wakeup = TimerWakeupSource::new(sleep_time);

    let had_button = critical_section::with(|cs| BUTTON.borrow_ref_mut(cs).take().is_some());

    log::info!("Going to sleep for {:?}...", sleep_time);

    if had_button {
        let pin: AnyPin<'static> = unsafe { AnyPin::steal(0) };
        let ext0 = Ext0WakeupSource::new(pin, WakeupLevel::Low);
        rtc.sleep_deep(&[&timer_wakeup, &ext0]);
    } else {
        rtc.sleep_deep(&[&timer_wakeup]);
    }
}

// Sets the boot type based on wakeup cause
pub(crate) fn apply_wakeup_boot_type() {
    match wakeup_cause() {
        // GPIO0 button was pressed
        SleepSource::Ext0 => BootType::set_boot_type(BootType::Config),
        // Timer expired
        SleepSource::Timer => BootType::set_boot_type(BootType::Display),
        // For other sources (like Undefined/Software Reset), we keep the current state
        _ => {}
    }
}

#[handler]
#[ram]
pub fn handler() {
    critical_section::with(|cs| {
        BUTTON
            .borrow_ref_mut(cs)
            .as_mut()
            .unwrap()
            .clear_interrupt();
    });
    BOOT_TYPES
        .fetch_update(
            core::sync::atomic::Ordering::Relaxed,
            core::sync::atomic::Ordering::Relaxed,
            |x| {
                Some(match BootType::swap_type(x) {
                    BootType::Display => BootType::Config as u8,
                    BootType::Config => BootType::Display as u8,
                })
            },
        )
        .unwrap();
    esp_hal::system::software_reset();
}
