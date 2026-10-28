// Thin Objective-C bridge. No audio or screen content is written to disk.
#import <AVFoundation/AVFoundation.h>
#import <CoreAudio/CoreAudio.h>
#import <CoreMedia/CoreMedia.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#include <math.h>
#include "capture.h"

static NSString *deviceString(AudioDeviceID device, AudioObjectPropertySelector selector) {
    AudioObjectPropertyAddress address = {selector, kAudioObjectPropertyScopeGlobal,
                                          kAudioObjectPropertyElementMain};
    CFStringRef value = NULL;
    UInt32 size = sizeof(value);
    if (AudioObjectGetPropertyData(device, &address, 0, NULL, &size, &value) != noErr || !value)
        return @"";
    return CFBridgingRelease(value);
}

void mc_audio_devices(bool input, void *context, MCDevice callback) {
    @autoreleasepool {
        AudioObjectPropertyAddress address = {input ? kAudioHardwarePropertyDefaultInputDevice
                                                    : kAudioHardwarePropertyDefaultOutputDevice,
            kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
        AudioDeviceID defaultDevice = kAudioObjectUnknown;
        UInt32 size = sizeof(defaultDevice);
        AudioObjectGetPropertyData(kAudioObjectSystemObject, &address, 0, NULL, &size, &defaultDevice);
        address.mSelector = kAudioHardwarePropertyDevices;
        if (AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &address, 0, NULL, &size) != noErr)
            return;
        NSMutableData *storage = [NSMutableData dataWithLength:size];
        if (AudioObjectGetPropertyData(kAudioObjectSystemObject, &address, 0, NULL, &size,
                                      storage.mutableBytes) != noErr) return;
        const AudioDeviceID *devices = storage.bytes;
        for (size_t i = 0; i < size / sizeof(AudioDeviceID); i++) {
            AudioObjectPropertyAddress streams = {kAudioDevicePropertyStreams,
                input ? kAudioDevicePropertyScopeInput : kAudioDevicePropertyScopeOutput,
                kAudioObjectPropertyElementMain};
            UInt32 streamSize = 0;
            if (AudioObjectGetPropertyDataSize(devices[i], &streams, 0, NULL, &streamSize) != noErr
                || streamSize == 0) continue;
            NSString *uid = deviceString(devices[i], kAudioDevicePropertyDeviceUID);
            NSString *name = deviceString(devices[i], kAudioObjectPropertyName);
            if (uid.length) callback(context, uid.UTF8String, name.UTF8String,
                                     devices[i] == defaultDevice);
        }
    }
}

@interface MCCapture : NSObject <SCStreamOutput, SCStreamDelegate,
                                 AVCaptureAudioDataOutputSampleBufferDelegate>
@property(nonatomic, strong) AVCaptureSession *mic;
@property(nonatomic, strong) SCStream *remote;
@property(nonatomic, strong) dispatch_queue_t queue;
@property(nonatomic) void *context;
@property(nonatomic) MCSamples samples;
@property(nonatomic) MCError error;
@property(nonatomic) BOOL stopping;
@property(nonatomic) int failureCode;
@property(nonatomic, copy) NSString *failure;
- (BOOL)startMic:(NSString *)uid;
- (BOOL)startRemote;
- (void)stop;
@end

@implementation MCCapture
- (instancetype)init {
    if ((self = [super init])) {
        _queue = dispatch_queue_create("io.github.amazingkenneth.meetcue.audio", DISPATCH_QUEUE_SERIAL);
        _failureCode = 3;
        _failure = @"Could not start audio capture";
    }
    return self;
}

- (BOOL)startMic:(NSString *)uid {
    AVAuthorizationStatus status = [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
    if (status == AVAuthorizationStatusNotDetermined) {
        dispatch_semaphore_t permission = dispatch_semaphore_create(0);
        [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio completionHandler:^(BOOL granted) {
            dispatch_semaphore_signal(permission);
        }];
        // Called from a worker, never the AppKit event loop. Permission
        // dialogs are user-paced, so do not time them out and leave capture
        // starting unexpectedly after the caller has already returned.
        dispatch_semaphore_wait(permission, DISPATCH_TIME_FOREVER);
        status = [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
    }
    if (status != AVAuthorizationStatusAuthorized) {
        self.failureCode = 1;
        self.failure = @"Allow MeetCue in System Settings > Privacy & Security > Microphone, then restart capture.";
        return NO;
    }
    AVCaptureDevice *selected = uid.length ? [AVCaptureDevice deviceWithUniqueID:uid] : nil;
    if (!selected || ![selected hasMediaType:AVMediaTypeAudio])
        selected = [AVCaptureDevice defaultDeviceWithMediaType:AVMediaTypeAudio];
    if (!selected) { self.failure = @"No microphone is connected"; return NO; }
    NSError *error = nil;
    AVCaptureDeviceInput *input = [AVCaptureDeviceInput deviceInputWithDevice:selected error:&error];
    if (!input) { self.failure = error.localizedDescription; return NO; }
    self.mic = [[AVCaptureSession alloc] init];
    AVCaptureAudioDataOutput *output = [[AVCaptureAudioDataOutput alloc] init];
    // Request float PCM; use the actual sample rate/channel layout carried
    // by every buffer rather than assuming the request was honoured.
    output.audioSettings = @{
        AVFormatIDKey: @(kAudioFormatLinearPCM), AVLinearPCMIsFloatKey: @YES,
        AVLinearPCMBitDepthKey: @32, AVLinearPCMIsNonInterleaved: @YES
    };
    [output setSampleBufferDelegate:self queue:self.queue];
    if (![self.mic canAddInput:input] || ![self.mic canAddOutput:output]) {
        self.failure = @"The microphone cannot deliver PCM audio";
        return NO;
    }
    [self.mic addInput:input];
    [self.mic addOutput:output];
    [[NSNotificationCenter defaultCenter] addObserver:self selector:@selector(micFailed:)
        name:AVCaptureSessionRuntimeErrorNotification object:self.mic];
    [self.mic startRunning];
    if (!self.mic.running) { self.failure = @"The microphone could not start"; return NO; }
    return YES;
}

- (BOOL)startRemote {
    __block SCShareableContent *content = nil;
    __block NSError *failure = nil;
    dispatch_semaphore_t ready = dispatch_semaphore_create(0);
    [SCShareableContent getShareableContentExcludingDesktopWindows:YES onScreenWindowsOnly:YES
        completionHandler:^(SCShareableContent *value, NSError *error) {
            content = value; failure = error; dispatch_semaphore_signal(ready);
        }];
    dispatch_semaphore_wait(ready, DISPATCH_TIME_FOREVER);
    if (!content) {
        // SCStreamErrorUserDeclined is -3801. Keep other failures distinct
        // from permission denial (e.g. a missing display).
        self.failureCode = failure.code == -3801 ? 2 : 3;
        self.failure = self.failureCode == 2
            ? @"Allow MeetCue in System Settings > Privacy & Security > Screen & System Audio Recording (Screen Recording on older macOS), then quit and reopen MeetCue."
            : (failure.localizedDescription ?: @"Cannot enumerate capture displays");
        return NO;
    }
    SCDisplay *display = content.displays.firstObject;
    if (!display) { self.failure = @"No display is available for system audio capture"; return NO; }
    SCContentFilter *filter = [[SCContentFilter alloc] initWithDisplay:display excludingWindows:@[]];
    SCStreamConfiguration *config = [[SCStreamConfiguration alloc] init];
    config.capturesAudio = YES;
    // Practice reads its own questions aloud and transcribes them through
    // this channel. The loopback self-test also plays sound in this process.
    config.excludesCurrentProcessAudio = NO;
    config.sampleRate = 48000;
    config.channelCount = 1;
    // Only subscribe to audio; no screen frames enter the application.
    config.width = 2; config.height = 2;
    config.minimumFrameInterval = CMTimeMake(1, 1);
    self.remote = [[SCStream alloc] initWithFilter:filter configuration:config delegate:self];
    NSError *error = nil;
    if (![self.remote addStreamOutput:self type:SCStreamOutputTypeAudio
                  sampleHandlerQueue:self.queue error:&error]) {
        self.failure = error.localizedDescription;
        return NO;
    }
    ready = dispatch_semaphore_create(0);
    [self.remote startCaptureWithCompletionHandler:^(NSError *error) {
        failure = error; dispatch_semaphore_signal(ready);
    }];
    dispatch_semaphore_wait(ready, DISPATCH_TIME_FOREVER);
    if (failure) {
        self.failureCode = failure.code == -3801 ? 2 : 3;
        self.failure = failure.localizedDescription;
        return NO;
    }
    return YES;
}

- (void)deliver:(CMSampleBufferRef)buffer channel:(int)channel {
    @synchronized(self) {
        if (self.stopping || !CMSampleBufferDataIsReady(buffer)) return;
        const AudioStreamBasicDescription *format = CMAudioFormatDescriptionGetStreamBasicDescription(
            CMSampleBufferGetFormatDescription(buffer));
        if (!format || format->mFormatID != kAudioFormatLinearPCM || format->mSampleRate <= 0
            || format->mChannelsPerFrame == 0) return;
        size_t listSize = 0;
        CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(buffer, &listSize, NULL, 0,
            kCFAllocatorDefault, kCFAllocatorDefault, 0, NULL);
        if (!listSize) return;
        NSMutableData *storage = [NSMutableData dataWithLength:listSize];
        AudioBufferList *list = storage.mutableBytes;
        CMBlockBufferRef block = NULL;
        OSStatus result = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(buffer, NULL,
            list, listSize, kCFAllocatorDefault, kCFAllocatorDefault,
            kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment, &block);
        if (result != noErr) return;
        size_t count = (size_t)CMSampleBufferGetNumSamples(buffer);
        NSMutableData *monoData = [NSMutableData dataWithLength:count * sizeof(float)];
        float *mono = monoData.mutableBytes;
        BOOL isFloat = (format->mFormatFlags & kAudioFormatFlagIsFloat) != 0;
        BOOL supported = (isFloat && format->mBitsPerChannel == 32)
            || (!isFloat && format->mBitsPerChannel == 16
                && (format->mFormatFlags & kAudioFormatFlagIsSignedInteger));
        if (supported) {
            for (UInt32 b = 0; b < list->mNumberBuffers; b++) {
                AudioBuffer audio = list->mBuffers[b];
                size_t channels = audio.mNumberChannels;
                size_t bytes = isFloat ? sizeof(float) : sizeof(int16_t);
                if (!audio.mData || channels == 0 || audio.mDataByteSize < count * channels * bytes) {
                    supported = NO; break;
                }
                for (size_t i = 0; i < count; i++) {
                    for (size_t c = 0; c < channels; c++) {
                        size_t index = i * channels + c;
                        float value = isFloat ? ((const float *)audio.mData)[index]
                            : ((const int16_t *)audio.mData)[index] / 32768.0f;
                        mono[i] += value / format->mChannelsPerFrame;
                    }
                }
            }
            if (supported) {
                double age = CMTimeGetSeconds(CMTimeSubtract(CMClockGetTime(CMClockGetHostTimeClock()),
                    CMSampleBufferGetPresentationTimeStamp(buffer))) * 1000.0;
                if (!isfinite(age) || age < 0 || age > 10000)
                    age = count * 1000.0 / format->mSampleRate;
                self.samples(self.context, channel, mono, count, (uint32_t)format->mSampleRate, age);
            }
        }
        if (block) CFRelease(block);
        if (!supported) self.error(self.context, channel, 3, "Unsupported device PCM format");
    }
}

- (void)captureOutput:(AVCaptureOutput *)output didOutputSampleBuffer:(CMSampleBufferRef)buffer
        fromConnection:(AVCaptureConnection *)connection {
    @autoreleasepool { [self deliver:buffer channel:0]; }
}

- (void)stream:(SCStream *)stream didOutputSampleBuffer:(CMSampleBufferRef)buffer
        ofType:(SCStreamOutputType)type {
    @autoreleasepool { if (type == SCStreamOutputTypeAudio) [self deliver:buffer channel:1]; }
}

- (void)stream:(SCStream *)stream didStopWithError:(NSError *)error {
    @synchronized(self) {
        if (!self.stopping) self.error(self.context, 1, error.code == -3801 ? 2 : 3,
                                       error.localizedDescription.UTF8String);
    }
}

- (void)micFailed:(NSNotification *)notification {
    @synchronized(self) {
        NSError *error = notification.userInfo[AVCaptureSessionErrorKey];
        if (!self.stopping) self.error(self.context, 0, 3, error.localizedDescription.UTF8String ?: "Microphone stopped");
    }
}

- (void)stop {
    // Serialize with every callback before the Rust context can be freed.
    @synchronized(self) { self.stopping = YES; }
    [[NSNotificationCenter defaultCenter] removeObserver:self];
    if (self.mic) {
        for (AVCaptureOutput *output in self.mic.outputs)
            if ([output isKindOfClass:AVCaptureAudioDataOutput.class])
                [(AVCaptureAudioDataOutput *)output setSampleBufferDelegate:nil queue:NULL];
        [self.mic stopRunning];
        self.mic = nil;
    }
    if (self.remote) {
        dispatch_semaphore_t stopped = dispatch_semaphore_create(0);
        [self.remote stopCaptureWithCompletionHandler:^(NSError *error) {
            dispatch_semaphore_signal(stopped);
        }];
        dispatch_semaphore_wait(stopped, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC));
        [self.remote removeStreamOutput:self type:SCStreamOutputTypeAudio error:NULL];
        self.remote = nil;
    }
    dispatch_sync(self.queue, ^{});
}
@end

int mc_audio_start(bool mic, bool remote, const char *uid, void *context,
                   MCSamples samples, MCError error, void **out, char *message, size_t capacity) {
    @autoreleasepool {
        *out = NULL;
        if (@available(macOS 13.0, *)) {
            MCCapture *capture = [[MCCapture alloc] init];
            capture.context = context; capture.samples = samples; capture.error = error;
            // Screen permission can require restarting the process. Check it
            // before opening the microphone so failure leaves neither open.
            if ((!remote || [capture startRemote])
                && (!mic || [capture startMic:[NSString stringWithUTF8String:uid]])) {
                *out = (__bridge_retained void *)capture;
                return 0;
            }
            [capture stop];
            snprintf(message, capacity, "%s", capture.failure.UTF8String ?: "Audio capture failed");
            return capture.failureCode;
        }
        snprintf(message, capacity, "MeetCue audio capture requires macOS 13 or later");
        return 4;
    }
}

void mc_audio_stop(void *handle) {
    if (!handle) return;
    @autoreleasepool {
        MCCapture *capture = (__bridge_transfer MCCapture *)handle;
        [capture stop];
    }
}

int mc_audio_tone(float frequency, uint32_t duration_ms) {
    @autoreleasepool {
        const uint32_t rate = 48000;
        uint32_t frames = (uint32_t)((uint64_t)rate * MIN(duration_ms, 10000) / 1000);
        AVAudioFormat *format = [[AVAudioFormat alloc] initStandardFormatWithSampleRate:rate channels:1];
        AVAudioPCMBuffer *buffer = [[AVAudioPCMBuffer alloc] initWithPCMFormat:format frameCapacity:frames];
        buffer.frameLength = frames;
        for (uint32_t i = 0; i < frames; i++)
            buffer.floatChannelData[0][i] = 0.2f * sinf(2 * M_PI * frequency * i / rate);
        AVAudioEngine *engine = [[AVAudioEngine alloc] init];
        AVAudioPlayerNode *player = [[AVAudioPlayerNode alloc] init];
        [engine attachNode:player];
        [engine connect:player to:engine.mainMixerNode format:format];
        NSError *error = nil;
        if (![engine startAndReturnError:&error]) return 3;
        [player scheduleBuffer:buffer completionHandler:nil];
        [player play];
        [NSThread sleepForTimeInterval:frames / (double)rate + 0.1];
        [engine stop];
        return 0;
    }
}
