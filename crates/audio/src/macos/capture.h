#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// All strings and sample arrays are borrowed only for the callback duration.
// Channel 0 = microphone, 1 = system sound. Error 1 = microphone permission,
// 2 = screen/audio permission, 3 = device/stream error, 4 = unsupported OS.
typedef void (*MCSamples)(void *, int, const float *, size_t, uint32_t, double);
typedef void (*MCError)(void *, int, int, const char *);
typedef void (*MCDevice)(void *, const char *, const char *, bool);

void mc_audio_devices(bool input, void *context, MCDevice callback);
int mc_audio_start(bool mic, bool remote, const char *uid, void *context,
                   MCSamples samples, MCError error, void **out,
                   char *message, size_t capacity);
// Stops and drains callbacks before returning; consumes the handle.
void mc_audio_stop(void *handle);
int mc_audio_tone(float frequency, uint32_t duration_ms);
