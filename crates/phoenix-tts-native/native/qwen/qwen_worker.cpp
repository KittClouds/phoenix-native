// PBN1 worker for Qwen3-TTS Base (qwentts.cpp). Same loopback protocol as the
// Breeze worker: one resident model, one request at a time, fresh synthesis
// per request, completion only after EOS and a barrier.
//
//   phoenix-qwen-worker <talker.gguf> <codec.gguf> <port> <nonce-hex>
//   phoenix-qwen-worker --enroll <talker.gguf> <codec.gguf> <ref.wav> <transcript.txt> <out.qwen>
//
// QWNV voice asset (little endian): "QWNV" | version=1 | rate=24000 | books |
// frames | transcript bytes | speaker dim | transcript | f32 speaker[dim] |
// i32 codes[books * frames] (row major, frames fastest).
#define NOMINMAX
#include <winsock2.h>
#include <ws2tcpip.h>
#include <windows.h>
#include "qwen.h"
#include "audio-io.h"
#include <algorithm>
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
constexpr uint32_t Ready = 0, Started = 1, Audio = 2, Eos = 3, Quiet = 4, Limit = 5, Failed = 6,
                   Request = 10, Barrier = 11, VoicedRequest = 12;
constexpr int SamplesPerFrame = 1920;  // 24 kHz / 12.5 Hz, as in the Breeze worker.
constexpr uint32_t MaxVoiceBytes = 512 * 1024;
struct Header { uint32_t kind; uint64_t request, sequence, value; };

void transfer(SOCKET socket, void *data, size_t n, bool write) {
    char *p = static_cast<char *>(data);
    while (n) {
        int result = write ? send(socket, p, static_cast<int>(n), 0) : recv(socket, p, static_cast<int>(n), 0);
        if (result <= 0) throw std::runtime_error("transport closed");
        p += result;
        n -= result;
    }
}
void put64(uint8_t *p, uint64_t v) { for (int i = 0; i < 8; i++) p[i] = uint8_t(v >> (8 * i)); }
uint64_t get64(const uint8_t *p) { uint64_t v = 0; for (int i = 0; i < 8; i++) v |= uint64_t(p[i]) << (8 * i); return v; }
uint32_t get32(const uint8_t *p) { return uint32_t(p[0]) | uint32_t(p[1]) << 8 | uint32_t(p[2]) << 16 | uint32_t(p[3]) << 24; }
void put32(std::string &out, uint32_t v) { for (int i = 0; i < 4; i++) out.push_back(char(v >> (8 * i))); }
Header read_header(SOCKET socket) {
    std::array<uint8_t, 32> b{};
    transfer(socket, b.data(), b.size(), false);
    if (memcmp(b.data(), "PBN1", 4)) throw std::runtime_error("magic");
    return {get32(b.data() + 4), get64(b.data() + 8), get64(b.data() + 16), get64(b.data() + 24)};
}
void emit(SOCKET socket, Header h) {
    std::array<uint8_t, 32> b{};
    memcpy(b.data(), "PBN1", 4);
    for (int i = 0; i < 4; i++) b[4 + i] = uint8_t(h.kind >> (i * 8));
    put64(b.data() + 8, h.request);
    put64(b.data() + 16, h.sequence);
    put64(b.data() + 24, h.value);
    transfer(socket, b.data(), b.size(), true);
}
int hex(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    throw std::runtime_error("nonce");
}

qt_context *load(const char *talker, const char *codec) {
    qt_log_set([](qt_log_level, const char *, void *) {}, nullptr);
    qt_init_params params;
    qt_init_default_params(&params);
    params.talker_path = talker;
    params.codec_path = codec;
    // Short decode chunks bound peak GPU memory (QT_MAX_CTX bounds the KV).
    params.codec_chunk_sec = 4.0f;
    params.max_batch = 1;
    qt_context *q = qt_init(&params);
    if (!q || std::string(qt_model_type(q)) != "base") return nullptr;
    return q;
}

// Parsed QWNV payload; pointers borrow the buffer.
struct Voice {
    std::string transcript;
    const float *spk = nullptr;
    int spk_dim = 0;
    const int32_t *codes = nullptr;
    int frames = 0;
    int books = 0;
};
bool parse_voice(const std::vector<uint8_t> &b, int books_expected, Voice &v) {
    if (b.size() < 28 || memcmp(b.data(), "QWNV", 4)) return false;
    const uint32_t version = get32(&b[4]), rate = get32(&b[8]), books = get32(&b[12]),
                   frames = get32(&b[16]), text = get32(&b[20]), dim = get32(&b[24]);
    if (version != 1 || rate != 24000 || int(books) != books_expected || !frames || frames > 375 ||
        !text || text > 16384 || !dim || dim > 4096) return false;
    const size_t need = 28 + size_t(text) + size_t(dim) * 4 + size_t(books) * frames * 4;
    if (b.size() != need) return false;
    v.transcript.assign(reinterpret_cast<const char *>(&b[28]), text);
    v.spk = reinterpret_cast<const float *>(&b[28 + text]);
    v.spk_dim = int(dim);
    v.codes = reinterpret_cast<const int32_t *>(&b[28 + text + size_t(dim) * 4]);
    v.frames = int(frames);
    v.books = int(books);
    for (int i = 0; i < v.spk_dim; i++) if (!std::isfinite(v.spk[i])) return false;
    for (size_t i = 0; i < size_t(books) * frames; i++) if (v.codes[i] < 0 || v.codes[i] >= 2048) return false;
    return true;
}

int enroll(int argc, char **argv) {
    if (argc != 7) return 1;
    qt_context *q = load(argv[2], argv[3]);
    if (!q) return 3;
    int n = 0;
    float *audio = audio_read_mono(argv[4], 24000, &n);
    if (!audio || n < 24000 || n > 24000 * 30) { free(audio); qt_free(q); return 4; }
    FILE *tf = fopen(argv[5], "rb");
    if (!tf) { free(audio); qt_free(q); return 4; }
    std::string transcript;
    char buf[4096];
    size_t got;
    while ((got = fread(buf, 1, sizeof buf, tf)) > 0) transcript.append(buf, got);
    fclose(tf);
    if (transcript.empty() || transcript.size() > 16384) { free(audio); qt_free(q); return 4; }
    // Clone mode continues from the reference, so a recording that stops
    // mid-sound makes every generated line open by finishing that sound (a
    // short grunt before the words). Refuse it, and always end the reference
    // in half a second of silence.
    float peak = 0.0f;
    for (int i = 0; i < n; i++) peak = std::max(peak, std::fabs(audio[i]));
    const int tail = 24000 * 60 / 1000;
    double energy = 0.0;
    for (int i = n - tail; i < n; i++) energy += double(audio[i]) * audio[i];
    const double tail_rms = std::sqrt(energy / tail);
    if (peak <= 0.0f || tail_rms > 0.05 * peak) { free(audio); qt_free(q); return 7; }
    std::vector<float> padded(audio, audio + n);
    free(audio);
    padded.resize(size_t(n) + 12000, 0.0f);
    qt_voice_ref ref = {};
    const qt_status rc = qt_extract_voice_ref(q, padded.data(), int(padded.size()), &ref);
    if (rc != QT_STATUS_OK || ref.ref_T < 1 || ref.ref_T > 375) { qt_voice_ref_free(&ref); qt_free(q); return 5; }
    std::string out = "QWNV";
    put32(out, 1);
    put32(out, 24000);
    put32(out, uint32_t(ref.num_codebooks));
    put32(out, uint32_t(ref.ref_T));
    put32(out, uint32_t(transcript.size()));
    put32(out, uint32_t(ref.ref_spk_dim));
    out += transcript;
    out.append(reinterpret_cast<const char *>(ref.ref_spk_emb), size_t(ref.ref_spk_dim) * 4);
    out.append(reinterpret_cast<const char *>(ref.ref_codes), size_t(ref.num_codebooks) * ref.ref_T * 4);
    qt_voice_ref_free(&ref);
    qt_free(q);
    FILE *of = fopen(argv[6], "wb");
    if (!of || fwrite(out.data(), 1, out.size(), of) != out.size()) { if (of) fclose(of); return 6; }
    return fclose(of) == 0 ? 0 : 6;
}
}  // namespace

int main(int argc, char **argv) {
    SetErrorMode(SEM_NOGPFAULTERRORBOX | SEM_FAILCRITICALERRORS);
    if (argc >= 2 && std::string(argv[1]) == "--enroll") return enroll(argc, argv);
    if (argc != 5) return 1;
    SOCKET socket = INVALID_SOCKET;
    try {
        WSADATA data{};
        if (WSAStartup(MAKEWORD(2, 2), &data)) return 2;
        socket = ::socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
        if (socket == INVALID_SOCKET) return 2;
        sockaddr_in address{};
        address.sin_family = AF_INET;
        address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        int port = std::stoi(argv[3]);
        if (port < 1 || port > 65535) return 2;
        address.sin_port = htons(uint16_t(port));
        int bound = 16 * 1024;
        if (setsockopt(socket, SOL_SOCKET, SO_SNDBUF, reinterpret_cast<char *>(&bound), sizeof(bound))) return 2;
        if (connect(socket, reinterpret_cast<sockaddr *>(&address), sizeof(address))) return 2;
        if (strlen(argv[4]) != 32) return 2;
        std::array<uint8_t, 16> nonce{};
        for (int i = 0; i < 16; i++) nonce[i] = uint8_t(hex(argv[4][2 * i]) * 16 + hex(argv[4][2 * i + 1]));
        transfer(socket, nonce.data(), nonce.size(), true);
        qt_context *q = load(argv[1], argv[2]);
        if (!q) return 3;
        const int books = qt_num_codebooks(q);
        emit(socket, {Ready, 0, 0, 24000});
        uint64_t previous = 0;
        for (;;) {
            Header h = read_header(socket);
            if (h.kind != VoicedRequest || h.request <= previous || h.sequence > UINT32_MAX ||
                h.value < SamplesPerFrame || h.value > 1'440'000) return 4;
            previous = h.request;
            std::array<uint8_t, 8> lengths{};
            transfer(socket, lengths.data(), lengths.size(), false);
            const auto text_size = get32(lengths.data()), direction_size = get32(lengths.data() + 4);
            if (!text_size || text_size > 16384 || direction_size > 4096) return 4;
            std::string text(text_size, '\0'), instruction(direction_size, '\0');
            transfer(socket, &text[0], text.size(), false);
            if (!instruction.empty()) transfer(socket, &instruction[0], instruction.size(), false);
            std::array<uint8_t, 4> size{};
            transfer(socket, size.data(), size.size(), false);
            const uint32_t n = get32(size.data());
            if (n < 28 || n > MaxVoiceBytes) return 4;
            std::vector<uint8_t> payload(n);
            transfer(socket, payload.data(), n, false);
            Voice voice;
            if (!parse_voice(payload, books, voice)) return 4;
            // Base clones take no style instruction; a direction is refused.
            if (!instruction.empty()) { emit(socket, {Failed, h.request, 0, 1}); continue; }
            const int max_new = int(h.value / SamplesPerFrame);
            // Context: ~250 prompt tokens plus the audio frames, as budgeted.
            emit(socket, {Started, h.request, 0, uint64_t(std::min(2048, max_new + 300))});
            qt_tts_params params;
            qt_tts_default_params(&params);
            params.text = text.c_str();
            params.lang = "english";
            params.seed = int64_t(uint32_t(h.sequence));
            params.max_new_tokens = max_new;
            params.ref_spk_emb = voice.spk;
            params.ref_spk_dim = voice.spk_dim;
            params.ref_codes = voice.codes;
            params.ref_T = voice.frames;
            params.ref_text = voice.transcript.c_str();
            qt_audio audio = {};
            const qt_status rc = qt_synthesize(q, &params, &audio);
            if (rc != QT_STATUS_OK || audio.n_samples <= 0) {
                qt_audio_free(&audio);
                emit(socket, {Failed, h.request, 1, 0});
                continue;
            }
            uint64_t frames = 0, sequence = 1;
            bool invalid = false;
            std::array<uint8_t, 4096> pcm{};
            const int total = std::min<int>(audio.n_samples, int(h.value));
            for (int at = 0; at < total && !invalid;) {
                const int count = std::min(2048, total - at);
                for (int i = 0; i < count; i++) {
                    const float x = audio.samples[at + i];
                    if (!std::isfinite(x)) { invalid = true; break; }
                    const int value = int(std::lround(std::clamp(x, -1.0f, 1.0f) * 32767.0f));
                    const uint16_t bits = uint16_t(int16_t(value));
                    pcm[size_t(i) * 2] = uint8_t(bits);
                    pcm[size_t(i) * 2 + 1] = uint8_t(bits >> 8);
                }
                if (invalid) break;
                emit(socket, {Audio, h.request, sequence++, uint64_t(count)});
                transfer(socket, pcm.data(), size_t(count) * 2, true);
                frames += count;
                at += count;
            }
            // Reaching the frame budget means no EOS was produced.
            const bool limited = audio.n_samples >= max_new * SamplesPerFrame;
            qt_audio_free(&audio);
            if (invalid || limited || !frames) {
                emit(socket, {limited ? Limit : Failed, h.request, sequence, frames});
                continue;
            }
            emit(socket, {Eos, h.request, sequence, frames});
            const auto barrier = read_header(socket);
            if (barrier.kind != Barrier || barrier.request != h.request || barrier.sequence != sequence ||
                barrier.value != frames) return 4;
            emit(socket, {Quiet, h.request, sequence + 1, frames});
        }
    } catch (...) {
        if (socket != INVALID_SOCKET) closesocket(socket);
        WSACleanup();
        return 5;
    }
}
