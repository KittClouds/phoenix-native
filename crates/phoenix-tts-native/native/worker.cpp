// PBN1 worker. No HTTP, JSON, shared synthesis sessions or implicit voice anchors.
#define NOMINMAX
#include <winsock2.h>
#include <ws2tcpip.h>
#include <windows.h>
#include "breeze/generation.h"
#include "breeze/model.h"
#include <array>
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <stdexcept>
#include <string>
using namespace breeze;
namespace {
constexpr uint32_t Ready=0, Started=1, Audio=2, Eos=3, Quiet=4, Limit=5, Failed=6, Request=10, Barrier=11, VoicedRequest=12;
struct Header { uint32_t kind; uint64_t request, sequence, value; };
void transfer(SOCKET socket, void *data, size_t n, bool write) {
    char *p=static_cast<char*>(data);
    while (n) {
        int result=write?send(socket,p,static_cast<int>(n),0):recv(socket,p,static_cast<int>(n),0);
        if (result<=0) throw std::runtime_error("transport closed");
        p+=result; n-=result;
    }
}
void put64(uint8_t *p,uint64_t v) { for(int i=0;i<8;i++) p[i]=uint8_t(v>>(8*i)); }
uint64_t get64(const uint8_t *p) { uint64_t v=0; for(int i=0;i<8;i++) v|=uint64_t(p[i])<<(8*i); return v; }
uint32_t get32(const uint8_t *p) { return uint32_t(p[0]) | uint32_t(p[1])<<8 | uint32_t(p[2])<<16 | uint32_t(p[3])<<24; }
Header read_header(SOCKET socket) {
    std::array<uint8_t,32> b{}; transfer(socket,b.data(),b.size(),false);
    if (memcmp(b.data(),"PBN1",4)) throw std::runtime_error("magic");
    return {get32(b.data()+4),get64(b.data()+8),get64(b.data()+16),get64(b.data()+24)};
}
void emit(SOCKET socket,Header h) {
    std::array<uint8_t,32> b{}; memcpy(b.data(),"PBN1",4);
    for(int i=0;i<4;i++) b[4+i]=uint8_t(h.kind>>(i*8));
    put64(b.data()+8,h.request);put64(b.data()+16,h.sequence);put64(b.data()+24,h.value);
    transfer(socket,b.data(),b.size(),true);
}
int hex(char c) { if(c>='0'&&c<='9')return c-'0'; if(c>='a'&&c<='f')return c-'a'+10; throw std::runtime_error("nonce"); }
}
int main(int argc,char **argv) {
    if(argc!=4) return 1;
    SetErrorMode(SEM_NOGPFAULTERRORBOX | SEM_FAILCRITICALERRORS);
    SOCKET socket=INVALID_SOCKET;
    try {
        WSADATA data{}; if(WSAStartup(MAKEWORD(2,2),&data)) return 2;
        socket=::socket(AF_INET,SOCK_STREAM,IPPROTO_TCP);
        if(socket==INVALID_SOCKET) return 2;
        sockaddr_in address{}; address.sin_family=AF_INET; address.sin_addr.s_addr=htonl(INADDR_LOOPBACK);
        int port=std::stoi(argv[2]); if(port<1||port>65535) return 2; address.sin_port=htons(uint16_t(port));
        int bound=16*1024; if(setsockopt(socket,SOL_SOCKET,SO_SNDBUF,reinterpret_cast<char*>(&bound),sizeof(bound))) return 2;
        if(connect(socket,reinterpret_cast<sockaddr*>(&address),sizeof(address))) return 2;
        if(strlen(argv[3])!=32) return 2;
        std::array<uint8_t,16> nonce{}; for(int i=0;i<16;i++) nonce[i]=uint8_t(hex(argv[3][2*i])*16+hex(argv[3][2*i+1]));
        transfer(socket,nonce.data(),nonce.size(),true);
        BreezeModel model;
        if(!model.load(argv[1],true) || !model.backend.is_gpu || model.cfg.sample_rate!=24000 || model.cfg.samples_per_frame!=1920) return 3;
        MimiCodec codec; codec.init(model);
        emit(socket,{Ready,0,0,24000});
        uint64_t previous=0;
        for(;;) {
            Header h=read_header(socket);
            if((h.kind!=Request && h.kind!=VoicedRequest) || h.request<=previous || h.sequence>UINT32_MAX || h.value<1920 || h.value>1'440'000) return 4;
            previous=h.request;
            std::array<uint8_t,8> lengths{}; transfer(socket,lengths.data(),lengths.size(),false);
            auto text_size=get32(lengths.data()), direction_size=get32(lengths.data()+4);
            if(!text_size || text_size>16384 || direction_size>4096) return 4;
            std::string text(text_size,'\0'),instruction(direction_size,'\0');
            transfer(socket,text.data(),text.size(),false); transfer(socket,instruction.data(),instruction.size(),false);
            GenRequest request; request.text=text; request.instruction=instruction; request.seed=static_cast<int>(uint32_t(h.sequence));
            if(h.kind==VoicedRequest) {
                std::array<uint8_t,4> size{}; transfer(socket,size.data(),size.size(),false);
                uint32_t n=get32(size.data()); if(n<24 || n>512*1024) return 4;
                std::vector<uint8_t> voice(n); transfer(socket,voice.data(),n,false);
                auto p=voice.data();
                const uint32_t books=get32(p+12), frames=get32(p+16), transcript=get32(p+20);
                if(memcmp(p,"BRZV",4) || get32(p+4)!=1 || get32(p+8)!=24000 || books!=16 ||
                    books!=uint32_t(model.cfg.num_codebooks) || !frames || frames>375 || !transcript || transcript>16384 ||
                    n!=24+transcript+frames*books*4) return 4;
                request.ref_text.assign(reinterpret_cast<char*>(p+24),transcript);
                request.ref_frames=static_cast<int>(frames); request.ref_codes.resize(frames*books);
                for(size_t i=0;i<request.ref_codes.size();i++) {
                    uint32_t code=get32(p+24+transcript+i*4);
                    if(code>=uint32_t(model.cfg.codec_codebook_size)) return 4;
                    request.ref_codes[i]=static_cast<int>(code);
                }
            }
            request.split_chars=0; request.cfg_scale=1; request.max_new_tokens=static_cast<int>(h.value/1920);
            // Match the text + reference audio + audio EOS conditional prompt.
            auto tokens=model.tok.encode("[S0]<ins_bos>"+instruction+"<ins_eos>"+text,true).size();
            if(request.ref_frames) tokens+=model.tok.encode("[S0]"+request.ref_text,true).size()+request.ref_frames+1;
            if(tokens==0 || tokens+request.max_new_tokens+8>2048) { emit(socket,{Failed,h.request,0,1}); continue; }
            emit(socket,{Started,h.request,0,tokens+uint64_t(request.max_new_tokens)+8});
            uint64_t frames=0,sequence=1; bool invalid=false;
            GenTimings timings; GenSession session; session.begin(model,codec,request);
            // Fresh session for every request: the cache key has no hidden history.
            bool ok=session.speak(text,[&](const float *samples,int count) {
                if(count<0 || uint64_t(count)>h.value-frames) { invalid=true; return false; }
                std::array<uint8_t,4096> pcm{};
                for(int at=0;at<count;) {
                    const int n=std::min(2048,count-at);
                    for(int i=0;i<n;i++) {
                        const float x=samples[at+i]; if(!std::isfinite(x)) { invalid=true; return false; }
                        int value=static_cast<int>(std::round(std::clamp(x,-1.0f,1.0f)*32767.0f));
                        uint16_t bits=static_cast<uint16_t>(static_cast<int16_t>(value));
                        pcm[i*2]=uint8_t(bits);pcm[i*2+1]=uint8_t(bits>>8);
                    }
                    emit(socket,{Audio,h.request,sequence++,uint64_t(n)}); transfer(socket,pcm.data(),size_t(n)*2,true);
                    frames+=n;at+=n;
                }
                return true;
            },&timings);
            if(!ok || invalid || !frames || timings.frames<0 || frames!=uint64_t(timings.frames)*1920 || timings.finish!=GenTimings::Finish::Eos) {
                emit(socket,{timings.finish==GenTimings::Finish::Limit?Limit:Failed,h.request,sequence,frames});
                continue;
            }
            emit(socket,{Eos,h.request,sequence,frames});
            const auto barrier=read_header(socket);
            if(barrier.kind!=Barrier || barrier.request!=h.request || barrier.sequence!=sequence || barrier.value!=frames) return 4;
            emit(socket,{Quiet,h.request,sequence+1,frames});
        }
    } catch(...) {
        if(socket!=INVALID_SOCKET) closesocket(socket);
        WSACleanup(); return 5;
    }
}
