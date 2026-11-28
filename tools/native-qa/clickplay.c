/* The recorder's reference (phase4.py calibrate): a PipeWire stream playing a 4 ms burst every
 * 0.5 s that prints, for each, the CLOCK_MONOTONIC time PipeWire says it is heard (the cycle's
 * time + pw_time.delay + its place in the buffer). The recorder's onsets less these: what the
 * recorder adds (one graph period), taken off the app's offsets.
 *   cc -O2 -o clickplay clickplay.c $(pkg-config --cflags --libs libpipewire-0.3) -lm */
#include <stdio.h>
#include <stdlib.h>
#include <math.h>
#include <time.h>
#include <pipewire/pipewire.h>
#include <spa/param/audio/format-utils.h>
struct d { struct pw_main_loop *loop; struct pw_stream *s; uint64_t pos; double until; };
static double mono(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec+t.tv_nsec/1e9;}
static void proc(void *u){ struct d *d=u; struct pw_buffer *b=pw_stream_dequeue_buffer(d->s); if(!b) return;
 struct pw_time t; pw_stream_get_time_n(d->s,&t,sizeof t);
 float *o=b->buffer->datas[0].data; uint32_t n=b->requested?b->requested:256; uint32_t max=b->buffer->datas[0].maxsize/8; if(n>max)n=max;
 for(uint32_t i=0;i<n;i++){ uint64_t p=d->pos+i; uint64_t k=p%24000; float v= k<192? 0.8f*sinf(2*M_PI*2000*k/48000.0f):0; o[2*i]=v;o[2*i+1]=v;
   if(k==0 && p>0){ int64_t heard=t.now + (int64_t)((t.delay + (int64_t)i)*1e9/48000.0) + (int64_t)(t.queued/8*1e9/48000.0); printf("%lld %lld %lld\n",(long long)heard,(long long)t.delay,(long long)t.queued); fflush(stdout);} }
 d->pos+=n; b->buffer->datas[0].chunk->offset=0; b->buffer->datas[0].chunk->stride=8; b->buffer->datas[0].chunk->size=n*8; pw_stream_queue_buffer(d->s,b);
 if(mono()>d->until) pw_main_loop_quit(d->loop);}
static const struct pw_stream_events ev={PW_VERSION_STREAM_EVENTS,.process=proc};
int main(int c,char**v){ struct d d={0}; pw_init(&c,&v); d.until=mono()+atof(v[1]); d.loop=pw_main_loop_new(NULL);
 d.s=pw_stream_new_simple(pw_main_loop_get_loop(d.loop),"qa-play",pw_properties_new(PW_KEY_MEDIA_TYPE,"Audio",PW_KEY_MEDIA_CATEGORY,"Playback",PW_KEY_NODE_NAME,"qa-play",PW_KEY_NODE_AUTOCONNECT,"false","adapter.auto-port-config","{ mode = dsp monitor = false position = preserve }",NULL),&ev,&d);
 uint8_t buf[1024]; struct spa_pod_builder bl=SPA_POD_BUILDER_INIT(buf,sizeof buf); const struct spa_pod *p[1];
 p[0]=spa_format_audio_raw_build(&bl,SPA_PARAM_EnumFormat,&SPA_AUDIO_INFO_RAW_INIT(.format=SPA_AUDIO_FORMAT_F32,.channels=2,.rate=48000,.position={SPA_AUDIO_CHANNEL_FL,SPA_AUDIO_CHANNEL_FR}));
 pw_stream_connect(d.s,PW_DIRECTION_OUTPUT,PW_ID_ANY,PW_STREAM_FLAG_MAP_BUFFERS|PW_STREAM_FLAG_RT_PROCESS,p,1);
 pw_main_loop_run(d.loop); return 0;}
