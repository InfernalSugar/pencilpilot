/*
 * A recorder for the sound QA: records a PipeWire sink's monitor (on a private PipeWire
 * with no devices, see phase4.py) and writes each buffer with the graph's clock, so every
 * sample has the CLOCK_MONOTONIC time the sink played it.
 *
 *   cc -O2 -o sinkclock sinkclock.c $(pkg-config --cflags --libs libpipewire-0.3) -lm
 *   PIPEWIRE_RUNTIME_DIR=<private> ./sinkclock <out.bin> <seconds>
 *
 * The stream is called "qa-capture" (ports input_FL / input_FR); link the sink's monitor
 * ports to it (pw-link). <out.bin>: for each buffer, int64 cycle time (ns, CLOCK_MONOTONIC),
 * uint64 ticks, uint32 rate, uint32 frames, then frames x 2 float32 (L, R).
 */
#include <math.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

#include <pipewire/pipewire.h>
#include <spa/param/audio/format-utils.h>

struct data {
	struct pw_main_loop *loop;
	struct pw_stream *stream;
	FILE *out;
	double until;
};

static double mono_s(void)
{
	struct timespec ts;
	clock_gettime(CLOCK_MONOTONIC, &ts);
	return ts.tv_sec + ts.tv_nsec / 1e9;
}

static void on_process(void *userdata)
{
	struct data *d = userdata;
	struct pw_time t;
	pw_stream_get_time_n(d->stream, &t, sizeof(t));
	uint32_t rate = t.rate.denom ? t.rate.denom : 48000;
	struct pw_buffer *b;
	/* every buffer queued (not only one a cycle: a recorder that fell behind would stay
	 * behind, and its times would be late by what's queued); each buffer's time from its
	 * own ticks (when the graph made it), against this cycle's ticks and time */
	while ((b = pw_stream_dequeue_buffer(d->stream)) != NULL) {
		struct spa_buffer *buf = b->buffer;
		float *samples = buf->datas[0].data;
		if (samples != NULL) {
			uint32_t n = buf->datas[0].chunk->size / (2 * sizeof(float));
			int64_t now = t.now;
			uint64_t ticks = b->time ? b->time : t.ticks;
			if (b->time && t.ticks >= b->time)
				now = t.now - (int64_t)((t.ticks - b->time) * 1000000000ull / rate);
			fwrite(&now, sizeof(now), 1, d->out);
			fwrite(&ticks, sizeof(ticks), 1, d->out);
			fwrite(&rate, sizeof(rate), 1, d->out);
			fwrite(&n, sizeof(n), 1, d->out);
			fwrite((char *)samples + buf->datas[0].chunk->offset, sizeof(float), n * 2, d->out);
		}
		pw_stream_queue_buffer(d->stream, b);
	}
	if (mono_s() > d->until)
		pw_main_loop_quit(d->loop);
}

static const struct pw_stream_events stream_events = {
	PW_VERSION_STREAM_EVENTS,
	.process = on_process,
};

static void do_quit(void *userdata, int signal_number)
{
	struct data *d = userdata;
	(void)signal_number;
	pw_main_loop_quit(d->loop);
}

int main(int argc, char *argv[])
{
	struct data d = {0};
	if (argc != 3) {
		fprintf(stderr, "usage: sinkclock <out.bin> <seconds>\n");
		return 2;
	}
	pw_init(&argc, &argv);
	d.out = fopen(argv[1], "wb");
	if (!d.out)
		return 1;
	d.until = mono_s() + atof(argv[2]);
	d.loop = pw_main_loop_new(NULL);
	pw_loop_add_signal(pw_main_loop_get_loop(d.loop), SIGINT, do_quit, &d);
	pw_loop_add_signal(pw_main_loop_get_loop(d.loop), SIGTERM, do_quit, &d);
	d.stream = pw_stream_new_simple(
		pw_main_loop_get_loop(d.loop), "qa-capture",
		pw_properties_new(PW_KEY_MEDIA_TYPE, "Audio", PW_KEY_MEDIA_CATEGORY, "Capture",
				  PW_KEY_MEDIA_ROLE, "Production", PW_KEY_NODE_NAME, "qa-capture",
				  PW_KEY_NODE_AUTOCONNECT, "false",
				  "adapter.auto-port-config", "{ mode = dsp monitor = false position = preserve }",
				  NULL),
		&stream_events, &d);
	uint8_t buffer[1024];
	struct spa_pod_builder bld = SPA_POD_BUILDER_INIT(buffer, sizeof(buffer));
	const struct spa_pod *params[1];
	params[0] = spa_format_audio_raw_build(
		&bld, SPA_PARAM_EnumFormat,
		&SPA_AUDIO_INFO_RAW_INIT(.format = SPA_AUDIO_FORMAT_F32, .channels = 2, .rate = 48000,
					 .position = {SPA_AUDIO_CHANNEL_FL, SPA_AUDIO_CHANNEL_FR}));
	pw_stream_connect(d.stream, PW_DIRECTION_INPUT, PW_ID_ANY,
			  PW_STREAM_FLAG_MAP_BUFFERS | PW_STREAM_FLAG_RT_PROCESS, params, 1);
	pw_main_loop_run(d.loop);
	pw_stream_destroy(d.stream);
	pw_main_loop_destroy(d.loop);
	fclose(d.out);
	pw_deinit();
	return 0;
}
