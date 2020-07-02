for i in `\ls new-api`; do clear; echo "#######
### ${i}
#######


"; diff -u old-api/${i} new-api/${i} | diffr; read; done
