import sys
import time
import mido
import evdev
from evdev import UInput, ecodes as e

def main():
    if len(sys.argv) < 2:
        print("Usage: python3 simple_roblox_autoplayer.py <midi_file>")
        sys.exit(1)
        
    filename = sys.argv[1]
    print(f"Loading {filename}...")
    
    mid = mido.MidiFile(filename)
    
    # Virtual piano layout
    keymap = {
        36: ('1', False), 37: ('1', True), 38: ('2', False), 39: ('2', True),
        40: ('3', False), 41: ('4', False), 42: ('4', True), 43: ('5', False),
        44: ('5', True), 45: ('6', False), 46: ('6', True), 47: ('7', False),
        48: ('8', False), 49: ('8', True), 50: ('9', False), 51: ('9', True),
        52: ('0', False), 53: ('q', False), 54: ('q', True), 55: ('w', False),
        56: ('w', True), 57: ('e', False), 58: ('e', True), 59: ('r', False),
        60: ('t', False), 61: ('t', True), 62: ('y', False), 63: ('y', True),
        64: ('u', False), 65: ('i', False), 66: ('i', True), 67: ('o', False),
        68: ('o', True), 69: ('p', False), 70: ('p', True), 71: ('a', False),
        72: ('s', False), 73: ('s', True), 74: ('d', False), 75: ('d', True),
        76: ('f', False), 77: ('g', False), 78: ('g', True), 79: ('h', False),
        80: ('h', True), 81: ('j', False), 82: ('j', True), 83: ('k', False),
        84: ('l', False), 85: ('l', True), 86: ('z', False), 87: ('z', True),
        88: ('x', False), 89: ('c', False), 90: ('c', True), 91: ('v', False),
        92: ('v', True), 93: ('b', False), 94: ('b', True), 95: ('n', False),
        96: ('m', False)
    }
    
    char_to_evdev = {
        '1': e.KEY_1, '2': e.KEY_2, '3': e.KEY_3, '4': e.KEY_4, '5': e.KEY_5,
        '6': e.KEY_6, '7': e.KEY_7, '8': e.KEY_8, '9': e.KEY_9, '0': e.KEY_0,
        'q': e.KEY_Q, 'w': e.KEY_W, 'e': e.KEY_E, 'r': e.KEY_R, 't': e.KEY_T,
        'y': e.KEY_Y, 'u': e.KEY_U, 'i': e.KEY_I, 'o': e.KEY_O, 'p': e.KEY_P,
        'a': e.KEY_A, 's': e.KEY_S, 'd': e.KEY_D, 'f': e.KEY_F, 'g': e.KEY_G,
        'h': e.KEY_H, 'j': e.KEY_J, 'k': e.KEY_K, 'l': e.KEY_L,
        'z': e.KEY_Z, 'x': e.KEY_X, 'c': e.KEY_C, 'v': e.KEY_V, 'b': e.KEY_B,
        'n': e.KEY_N, 'm': e.KEY_M
    }
    
    try:
        ui = UInput()
    except Exception as exc:
        print(f"Error initializing uinput (are you root/sudo?): {exc}")
        sys.exit(1)
        
    print("Switch to the Roblox window now! Starting in 3 seconds...")
    time.sleep(3)
    
    print("Playing...")
    for msg in mid.play():
        if msg.type == 'note_on' and msg.velocity > 0:
            if msg.note in keymap:
                char, shift = keymap[msg.note]
                evdev_key = char_to_evdev.get(char)
                if evdev_key:
                    if shift:
                        ui.write(e.EV_KEY, e.KEY_LEFTSHIFT, 1)
                        ui.syn()
                    ui.write(e.EV_KEY, evdev_key, 1)
                    ui.syn()
                    ui.write(e.EV_KEY, evdev_key, 0)
                    ui.syn()
                    if shift:
                        ui.write(e.EV_KEY, e.KEY_LEFTSHIFT, 0)
                        ui.syn()
                        
    print("Done!")
    ui.close()

if __name__ == "__main__":
    main()
