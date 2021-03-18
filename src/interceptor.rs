use crate::runner::TestDescriptor;
use std::sync::Once;

static INTERCEPTOR: Once = Once::new();

pub fn install_interceptor() {
    INTERCEPTOR.call_once(|| {
        intercept_test_main_static();
    });
}

fn intercept_test_main_static() {
    let ptr = rustc_test::test_main_static as *mut ();
    let target = test_main_static_intercepted as *const ();
    // 5 is the size of jmp instruction + offset
    let diff = isize::wrapping_sub(target as _, ptr as _) as i32 - 5;

    // Patch the test_main_static function so it instead jumps to our own function.
    // e9 00 00 00 00 jmp    5 <_main+0x5>
    unsafe {
        let diff_bytes = diff.to_le_bytes();
        let bytes = ptr as *mut u8;
        let _handle = region::protect_with_handle(bytes, 5, region::Protection::WRITE_EXECUTE)
            .expect("failed to modify page protection to intercept `test_main_static`");

        std::ptr::write(bytes.offset(0), 0xe9);
        std::ptr::write(bytes.offset(1), diff_bytes[0]);
        std::ptr::write(bytes.offset(2), diff_bytes[1]);
        std::ptr::write(bytes.offset(3), diff_bytes[2]);
        std::ptr::write(bytes.offset(4), diff_bytes[3]);
    }
}

fn test_main_static_intercepted(tests: &[&rustc_test::TestDescAndFn]) {
    let tests = tests
        .iter()
        .map(|v| *v as &dyn TestDescriptor)
        .collect::<Vec<&dyn TestDescriptor>>();
    crate::runner(&tests);
}
