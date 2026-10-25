# Unique Id Service

Obtain a unique id via DNS request.

```shell-session
$ dig -p 3000

; <<>> DiG 9.20.30 <<>> -p 3000
; (1 server found)
;; global options: +cmd
;; Got answer:
;; ->>HEADER<<- opcode: QUERY, status: NOERROR, id: 45205
;; flags: qr aa rd; QUERY: 1, ANSWER: 1, AUTHORITY: 0, ADDITIONAL: 0
;; WARNING: recursion requested but not available

;; QUESTION SECTION:
;.				IN	NS

;; ANSWER SECTION:
.			0	IN	TXT	"05ce7a15f2"

;; Query time: 7 msec
;; SERVER: 127.0.0.1#3000(127.0.0.1) (UDP)
;; WHEN: Wed Oct 21 13:03:58 UTC 2026
;; MSG SIZE  rcvd: 39

```
