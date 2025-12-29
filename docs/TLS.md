# TLS-Konzept für Airlift Node

Airlift Node nutzt aktuell `tiny_http` für HTTP-Streaming und Monitoring. Dieses
HTTP-Setup unterstützt kein natives TLS. Daher wird eine TLS-Terminierung über
einen Reverse-Proxy empfohlen. Das Konzept sieht vor, dass der Node lokal (oder
im privaten Netzwerk) unverschlüsselt läuft und der Proxy das TLS sowie die
öffentliche Erreichbarkeit übernimmt.

## Empfohlenes Setup: Reverse-Proxy mit TLS-Termination

**Ziel:** TLS/HTTPS gegenüber Clients, unverändertes HTTP intern.

**Bausteine:**

1. **Airlift Node** läuft lokal (z. B. `127.0.0.1:8087` für Monitoring/HTTP).
2. **Reverse-Proxy** (Nginx, Caddy, Traefik) übernimmt Zertifikate, HTTPS und
   Weiterleitung.
3. **Firewall** lässt nur den Proxy-Port (443) nach außen zu.

### Beispiel Nginx-Konfiguration

```nginx
server {
    listen 443 ssl;
    server_name audio.example.com;

    ssl_certificate     /etc/letsencrypt/live/audio.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/audio.example.com/privkey.pem;

    location /audio/ {
        proxy_pass http://127.0.0.1:8087;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $remote_addr;
        proxy_set_header X-Forwarded-Proto https;
    }

    location /metrics {
        proxy_pass http://127.0.0.1:8087;
    }
}
```

### Beispiel Caddy-Konfiguration

```caddy
audio.example.com {
    reverse_proxy 127.0.0.1:8087
}
```

## Betriebshinweise

- **Interner Zugriff:** Airlift Node sollte nur intern (localhost oder private IP)
  erreichbar sein.
- **Zertifikatsverwaltung:** Nutzt ACME (z. B. Let's Encrypt) im Proxy.
- **HTTP-Streams:** Für Live-Streams ist der Proxy transparent; es werden keine
  Header-Rewrites benötigt.

## Optional: Native TLS (zukünftige Option)

Falls zukünftig ein HTTP-Server mit TLS-Support integriert wird, kann die
TLS-Terminierung direkt im Node erfolgen. Bis dahin wird der Reverse-Proxy-Ansatz
als Standard empfohlen.
