// VITL Piano — native keystroke injection backend (C++)
//
// Creates a persistent kernel-level virtual keyboard via /dev/uinput and
// emits EV_KEY events. Works on both Wayland and X11. Exposed through a
// stable C ABI so it can be linked into the Rust binary via build.rs
// without any external crate dependencies.
//
// Thread safety: all writes are serialized with a mutex; init() is idempotent.

#include <fcntl.h>
#include <linux/input-event-codes.h>
#include <linux/uinput.h>
#include <mutex>
#include <cstring>
#include <cerrno>
#include <unistd.h>

extern "C" {

/// Logical keys understood by the backend (stable values — do not change).
enum VitlKey {
    VITL_KEY_A = 0, VITL_KEY_B, VITL_KEY_C, VITL_KEY_D, VITL_KEY_E,
    VITL_KEY_F, VITL_KEY_G, VITL_KEY_H, VITL_KEY_I, VITL_KEY_J,
    VITL_KEY_K, VITL_KEY_L, VITL_KEY_M, VITL_KEY_N, VITL_KEY_O,
    VITL_KEY_P, VITL_KEY_Q, VITL_KEY_R, VITL_KEY_S, VITL_KEY_T,
    VITL_KEY_U, VITL_KEY_V, VITL_KEY_W, VITL_KEY_X, VITL_KEY_Y,
    VITL_KEY_Z,
    VITL_KEY_1, VITL_KEY_2, VITL_KEY_3, VITL_KEY_4, VITL_KEY_5,
    VITL_KEY_6, VITL_KEY_7, VITL_KEY_8, VITL_KEY_9, VITL_KEY_0,
    VITL_KEY_SPACE,
    VITL_KEY_LEFTSHIFT,
    VITL_KEY_LEFTCTRL,
    VITL_KEY_LEFTALT,
    VITL_KEY_COUNT
};

namespace {

int g_ufd = -1;
bool g_failed = false;
std::mutex g_mutex;

unsigned int vitl_to_evdev(int key) {
    switch (key) {
        case VITL_KEY_A: return KEY_A;
        case VITL_KEY_B: return KEY_B;
        case VITL_KEY_C: return KEY_C;
        case VITL_KEY_D: return KEY_D;
        case VITL_KEY_E: return KEY_E;
        case VITL_KEY_F: return KEY_F;
        case VITL_KEY_G: return KEY_G;
        case VITL_KEY_H: return KEY_H;
        case VITL_KEY_I: return KEY_I;
        case VITL_KEY_J: return KEY_J;
        case VITL_KEY_K: return KEY_K;
        case VITL_KEY_L: return KEY_L;
        case VITL_KEY_M: return KEY_M;
        case VITL_KEY_N: return KEY_N;
        case VITL_KEY_O: return KEY_O;
        case VITL_KEY_P: return KEY_P;
        case VITL_KEY_Q: return KEY_Q;
        case VITL_KEY_R: return KEY_R;
        case VITL_KEY_S: return KEY_S;
        case VITL_KEY_T: return KEY_T;
        case VITL_KEY_U: return KEY_U;
        case VITL_KEY_V: return KEY_V;
        case VITL_KEY_W: return KEY_W;
        case VITL_KEY_X: return KEY_X;
        case VITL_KEY_Y: return KEY_Y;
        case VITL_KEY_Z: return KEY_Z;
        case VITL_KEY_1: return KEY_1;
        case VITL_KEY_2: return KEY_2;
        case VITL_KEY_3: return KEY_3;
        case VITL_KEY_4: return KEY_4;
        case VITL_KEY_5: return KEY_5;
        case VITL_KEY_6: return KEY_6;
        case VITL_KEY_7: return KEY_7;
        case VITL_KEY_8: return KEY_8;
        case VITL_KEY_9: return KEY_9;
        case VITL_KEY_0: return KEY_0;
        case VITL_KEY_SPACE:      return KEY_SPACE;
        case VITL_KEY_LEFTSHIFT:  return KEY_LEFTSHIFT;
        case VITL_KEY_LEFTCTRL:   return KEY_LEFTCTRL;
        case VITL_KEY_LEFTALT:    return KEY_LEFTALT;
        default: return KEY_RESERVED;
    }
}

inline int write_event(unsigned type, unsigned code, int value) {
    input_event ev{};
    ev.type = type;
    ev.code = code;
    ev.value = value;
    if (::write(g_ufd, &ev, sizeof(ev)) < 0) return -errno;
    return 0;
}

} // namespace

/// Create the virtual keyboard. Idempotent.
/// Returns 0 on success, negative errno-style error otherwise.
int vitl_macro_init(void) {
    std::lock_guard<std::mutex> lock(g_mutex);
    if (g_ufd >= 0) return 0;
    if (g_failed) return -EOPNOTSUPP;

    int fd = ::open("/dev/uinput", O_WRONLY | O_NONBLOCK);
    if (fd < 0) {
        g_failed = true;
        return -errno;
    }

    if (::ioctl(fd, UI_SET_EVBIT, EV_KEY) < 0 ||
        ::ioctl(fd, UI_SET_EVBIT, EV_SYN) < 0) {
        int err = -errno;
        ::close(fd);
        g_failed = true;
        return err;
    }

    // Register every key we can emit.
    for (int k = 0; k < VITL_KEY_COUNT; ++k) {
        unsigned int code = vitl_to_evdev(k);
        if (code != KEY_RESERVED &&
            ::ioctl(fd, UI_SET_KEYBIT, code) < 0) {
            int err = -errno;
            ::close(fd);
            g_failed = true;
            return err;
        }
    }

    uinput_setup usetup{};
    usetup.id.bustype = BUS_USB;
    usetup.id.vendor  = 0x5649; // "VI"
    usetup.id.product = 0x544C; // "TL"
    usetup.id.version = 1;
    std::strncpy(usetup.name, "VITL Piano Autoplayer", UINPUT_MAX_NAME_SIZE - 1);

    if (::ioctl(fd, UI_DEV_SETUP, &usetup) < 0 ||
        ::ioctl(fd, UI_DEV_CREATE) < 0) {
        int err = -errno;
        ::close(fd);
        g_failed = true;
        return err;
    }

    g_ufd = fd;
    return 0;
}

/// Send one key event (value: 1 = press, 0 = release, 2 = repeat).
/// Returns 0 on success, negative errno-style error otherwise.
int vitl_macro_write(int key, int value) {
    std::lock_guard<std::mutex> lock(g_mutex);
    if (g_ufd < 0) return -ENODEV;

    unsigned int code = vitl_to_evdev(key);
    if (code == KEY_RESERVED) return -EINVAL;

    int err = write_event(EV_KEY, code, value);
    if (err) return err;
    err = write_event(EV_SYN, SYN_REPORT, 0);
    return err;
}

/// True when the virtual keyboard is ready for use.
int vitl_macro_ready(void) {
    return g_ufd >= 0 ? 1 : 0;
}

} // extern "C"
