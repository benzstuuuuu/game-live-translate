#!/bin/sh
# usage: mic.sh <secs> <outfile>  -> host-clock pts_time + 1 kHz band RMS per 5 ms from the raw mic
perl -e "alarm $(($1+10)); exec @ARGV" ffmpeg -hide_banner -loglevel error -t "$1" -f avfoundation -i ":MacBook Air Microphone" -copyts \
  -af "bandpass=f=1000:width_type=q:w=8,asetnsamples=n=240,astats=metadata=1:reset=1,ametadata=print:key=lavfi.astats.Overall.RMS_level:file=$2" -f null -
