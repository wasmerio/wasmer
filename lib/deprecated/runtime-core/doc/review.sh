#!/bin/bash

for i in `\ls new-api | grep "${1-.}"`; do
    clear
    echo -e "#######\n### ${i}\n#######\n\n\n\n"
    diff -u old-api/${i} new-api/${i} | diffr

    echo -e "\n\n\n\nPress any key to diff the next file…"
    read
done
