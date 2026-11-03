# Replix – Features und Recherche

Stand: 3. November 2026. Die Angaben zu anderen Apps stammen meist von deren eigenen Seiten oder aus Vergleichsartikeln von Konkurrenten. Nimm sie als Herstellerangaben, nicht als getestet.

## Vergleichbare Apps

| App | Was es ist | Was auffällt |
| --- | --- | --- |
| **Medal** | Clip-App mit Cloud und Community | Replay 15 s bis 10 min (F8), Sprachbefehl „clip that“, Auto-Clip in 100+ Spielen, ganze Sessions mit Lesezeichen, Spiel/Mikro/Discord als eigene Spuren ohne virtuelle Kabel, Editor mit Trimmen, Text, Zeitlupe, Zoom und Untertiteln, Teilen-Link, Speicherlimit 1 bis 500 GB (älteste Clips werden gelöscht), Import fremder Aufnahmen, Bibliothek mit Filter nach Spiel/Datum/Labels |
| **Outplayed** (Overwolf) | Auto-Highlights über Spiel-Events | Modi: nur Highlights, ganzes Match oder ganze Session. Matches als Leiste oben, Supercut (Highlights zusammenschneiden), automatisches Aufräumen des Speichers. Braucht Overwolf, Werbung (Premium ohne) |
| **Steam-Spielaufnahme** | In Steam eingebaut | Hintergrundaufnahme (Standard 120 min, Länge und Speicherlimit einstellbar) oder manuell. Zeitleiste mit Markern: vom Spiel (Kills usw.), von Steam (Erfolge, Screenshots) und eigene per Hotkey. „Zum nächsten Event springen“. Pro Spiel eigene Länge/Qualität inkl. „Nicht aufnehmen“. Nimmt nur das Spiel auf, nicht den Desktop. Audiokanäle wählbar. Export als MP4, an anderes Gerät senden, temporärer Link |
| **NVIDIA App / AMD Adrenalin** | Treiber-Tools der Grafikkarte | Instant Replay (AMD bis 20 min, in 15-s-Schritten), automatische Highlights in einigen Spielen. AMD kann das Mikro als eigene Datei speichern. Kaum Bibliothek oder Editor |
| **Xbox Game Bar** | In Windows eingebaut | „Letzte 30 s“ mit Win+Alt+G, Aufnahme Win+Alt+R, Mikro Win+Alt+M. Läuft nur, wenn Windows ein Spiel erkennt |
| **SteelSeries Moments** | Clip-App von SteelSeries | Auto-Clip pro Spiel mit wählbaren Events (Headshot, Rundensieg, Clutch …), Marker auf der Zeitleiste, Favoriten mit Stern, Galerie nach Spiel und Tag, Trimmen ohne Qualitätsverlust, Montagen, Upload zu YouTube/Discord/Reddit/X. Chat ist nur getrennt, wenn er über ein eigenes Ausgabegerät läuft |
| **Insights Capture** | Clip-App mit Team-Review | Auto-Clip in sehr vielen Spielen, Lesezeichen auf der Zeitleiste, Clips zusammenführen, Kommentare mit Zeitstempel fürs Team |
| **OBS Studio** (Replay Buffer) | Profi-Aufnahmeprogramm | Ton pro Programm (ab Windows 10 2004), mehrere Spuren, alles einstellbar, aber kompliziert und ohne Bibliothek |
| **ClipFlow** (Open Source, MIT) | Rust + Tauri v2, wie Replix | Alt+C speichert, Puffer im Grafikspeicher, DXGI-Aufnahme, Hardware-Encoder (NVENC/AMF/QSV), Profile pro Spiel, Trimmen ohne Neu-Kodierung, Galerie mit Favoriten und Tags, erholt sich nach Auflösungswechsel oder Treiber-Reset, GIF-Export. Spiel und Mikro landen gemischt in einer Spur |
| **Ember Replay** (Open Source) | Rust + Tauri, nutzt den OBS-Kern (libobs) | Bleibt im Tray, F9 speichert, bis zu 5 Audioquellen mit eigener Lautstärke |
| **Cap** (Open Source, AGPL) | Bildschirmrekorder in Rust + Tauri | Kein Gaming-Tool, aber gute Referenz für Aufbau und Editor |

## Was Replix können sollte

### Muss (erste Version)

**Aufnahme**
- Replay-Puffer: die letzten N Sekunden (15 s bis 10 min), speichern mit Alt+C.
- Quelle wählen: ganzer Bildschirm, Spiel (automatisch erkannt) oder ein bestimmtes Programm.
- Session-Aufnahme: Start und Stopp mit Alt+R.
- Screenshot mit Alt+S.
- Jede Tonquelle als eigene Spur: Spiel, Mikrofon, Discord, Spotify, System. Das kann bisher nur Medal richtig, bei SteelSeries braucht es dafür eigene Ausgabegeräte. Das ist unser wichtigster Vorteil.
- Hardware-Encoder (NVENC, AMF, QSV) mit Software als Rückfall.
- Läuft im Tray, startet mit Windows, Puffer startet bei Spielbeginn.
- Kurze Rückmeldung beim Speichern (kleine Einblendung oder Ton), damit man weiß, dass es geklappt hat.
- Tastenkürzel frei belegbar.

**Bibliothek**
- Clips als Raster nach Datum, Filter nach Spiel, Bereiche für Clips, Screenshots, Sessions und Entwürfe.
- Spielname automatisch am Clip.
- Suche und Favoriten.
- Speicherlimit mit automatischem Aufräumen (älteste zuerst, Favoriten und Entwürfe bleiben).

**Editor**
- Trimmen, wenn möglich ohne Neu-Kodierung.
- Jede Spur stumm schalten und lauter oder leiser machen.
- Als Entwurf speichern oder als MP4 exportieren.
- Ein Bild aus dem Clip als Screenshot sichern.
- Export-Voreinstellung „Für Discord“ (klein genug für den Upload ohne Nitro).

### Soll (bald danach)
- Marker per Tastenkürzel während einer Session, im Editor zum nächsten Marker springen (wie Steam).
- Einstellungen pro Spiel: Länge, Qualität, „nicht aufnehmen“.
- Datenschutz: nur das Spiel aufnehmen, nie den Desktop; Programme ausschließen.
- Clip in die Zwischenablage kopieren oder direkt nach Discord ziehen.
- Export mit getrennten Spuren (MKV) für Schnittprogramme.
- Vorhandene Videos importieren.

### Später
- Auto-Clip über Spiel-Events (Kill, Tor …). Das ist viel Arbeit pro Spiel.
- Mehrere Clips zu einer Montage zusammenschneiden.
- Teilen per Link (braucht einen Server).
- Sprachbefehl „Clip that“.
- Text, Zeitlupe und Zoom im Editor, Hochformat für TikTok und Shorts.

### Bewusst nicht
- Community-Feed, Werbung, Kontopflicht, Abhängigkeit von Overwolf.

## Was wir von deren Oberflächen lernen

Screenshots liegen in `recherche/ui/`.

- **Steam** (`steam-zeitleiste.jpg`, `steam-bibliothek.jpg`): Die Zeitleiste mit kleinen Symbolen für Ereignisse ist die beste Idee. So findet man Momente, ohne das ganze Video anzusehen. Die Bibliothek gruppiert nach Spiel-Sessions mit Vorschaubildern daneben.
- **ClipFlow** (`clipflow.jpg`): Status, Galerie und alle Einstellungen auf einer Seite. Technisch stark, aber genau das Überladene, das du nicht willst.
- **Outplayed** (`outplayed.jpg`): Schmale Symbolleiste links, Matches als Leiste oben, das Video groß in der Mitte.
- **Medal**: Bibliothek als Startseite, Editor öffnet sich pro Clip, dazu viel Community (Feed, Profile). Ein brauchbarer Screenshot war auf der Website nicht abrufbar.

Für Replix heißt das: Bibliothek als Startseite (passt zu unseren Entwürfen), Marker auf der Zeitleiste übernehmen, Einstellungen in ein eigenes Fenster statt auf die Hauptseite.

## Technik-Hinweise (Windows)
- **Bild:** Windows Graphics Capture für Fenster und Spiele, DXGI Desktop Duplication für den ganzen Bildschirm (nutzt ClipFlow).
- **Ton pro Programm:** WASAPI Process Loopback, ab Windows 10 Version 2004 (dieselbe Technik wie OBS' „Anwendungsaudioaufnahme“).
- **Kodieren:** Media Foundation mit den Hardware-Encodern (wie ClipFlow) oder FFmpeg.
- **Mehrere Spuren:** Clips intern mit einer Spur pro Quelle speichern, beim Export mischen.

## Quellen
- Medal: [Funktionen](https://medal.tv/features), [Medal vs OBS](https://medal.tv/compare/medal-vs-obs), [Beste Clipping-Software](https://medal.tv/learn/best-clipping-software)
- Outplayed: [outplayed.tv](https://outplayed.tv), [Insights vs Outplayed](https://insights.gg/blog/insights-vs-outplayed), [Overwolf Support](https://support.overwolf.com/support/solutions/articles/9000188179)
- Steam: [Steam-Spielaufnahme](https://store.steampowered.com/gamerecording)
- AMD: [Adrenalin FAQ DH3-023](https://www.amd.com/en/resources/support-articles/faqs/DH3-023.html)
- Xbox Game Bar: [PCWorld](https://www.pcworld.com/article/394683/how-to-record-your-screen-in-windows-10.html), [ElevenForum](https://elevenforum.com/t/enable-or-disable-record-what-happened-for-gaming-captures-in-windows-11.17900/)
- SteelSeries Moments: [steelseries.com/gg/moments](https://steelseries.com/gg/moments), [Black Ops 6 Guide](https://steelseries.com/ko-kr/blog/how-to-capture-black-ops-6-clips-1203)
- OBS: [Application Audio Capture Guide](https://obsproject.com/kb/application-audio-capture-guide)
- ClipFlow: [github.com/jaimitus/ClipFlow](https://github.com/jaimitus/ClipFlow)
- Ember Replay: [github.com/halconrandom/Ember-Replay](https://github.com/halconrandom/Ember-Replay)
- Cap: [cap.so](https://cap.so/llms-full.txt)
