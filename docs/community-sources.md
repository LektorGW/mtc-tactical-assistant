# Publiczne dane społeczności MTC

Aplikacja zawiera 153 eksperymentalne profile pocisków dla 51 nazw uzbrojenia oraz katalog wymiarów 25 map. Oryginalne siedem szablonów lokalnej kalibracji pozostaje dostępne. Wszystkie liczby w imporcie pochodzą z kalkulatora gry MTC. Nie zastępujemy ich parametrami rzeczywistych broni o podobnych nazwach.

Źródło: [grand-hawk/artillery-calculator](https://github.com/grand-hawk/artillery-calculator), gałąź `dev`, commit [`39a7af7689524e57ca2326f3b998cf123df95426`](https://github.com/grand-hawk/artillery-calculator/tree/39a7af7689524e57ca2326f3b998cf123df95426), data commitu 2026-10-03. Moment pobrania zapisano w `data/weapons/community/manifest.json` w UTC. Snapshot jest lokalny; aplikacja nie łączy się z GitHub podczas rozgrywki.

## Zakres źródeł i jednostki

| Plik źródłowy | Wykorzystanie |
| --- | --- |
| [importedGuns.json](https://github.com/grand-hawk/artillery-calculator/blob/39a7af7689524e57ca2326f3b998cf123df95426/packages/mtc-artillery/src/config/importedGuns.json) | Nazwy pojazdów/pocisków i prędkości w konwencji kalkulatora gry |
| [guns.ts](https://github.com/grand-hawk/artillery-calculator/blob/39a7af7689524e57ca2326f3b998cf123df95426/packages/mtc-artillery/src/config/guns.ts) | Siedem dodatkowych rodzin uzbrojenia; importowana lista nadpisuje klucz o tej samej nazwie, zgodnie ze źródłem |
| [math.ts](https://github.com/grand-hawk/artillery-calculator/blob/39a7af7689524e57ca2326f3b998cf123df95426/packages/mtc-artillery/src/utils/math.ts) | Model bez oporu, `G = 9.8 × 1.8 = 17.64`; `1 stud = 0.36 m gry` |
| [maps.ts](https://github.com/grand-hawk/artillery-calculator/blob/39a7af7689524e57ca2326f3b998cf123df95426/packages/mtc-artillery/src/config/maps.ts) | Pełne wymiary map w studs, przeliczone na metry gry; nazwy i historyczna flaga rotacji |
| [README.md](https://github.com/grand-hawk/artillery-calculator/blob/39a7af7689524e57ca2326f3b998cf123df95426/README.md) | Źródło odnosi się do Roblox place `9520328730` |

`game_meters` oznacza konwencję tego kalkulatora, a nie sprawdzone jednostki aktualnej wersji Roblox. Przykład BM-21 Grad: `180 m gry/s`, `17.64 m gry/s²`. Dystans mapy, różnica wysokości i prędkości celu muszą używać tej samej jednostki. Lokalny profil domyślnie zachowuje `game_units` — własne, wzajemnie spójne jednostki użytkownika.

README źródła wskazuje place `9520328730`. Zgodność danych z aktualnym MTC, innym place ID, zmianami fizyki i wersją map nie została potwierdzona. Commit i data pobrania nie są wersją gry. Flaga `inRotation` jest stanem konfiguracji źródła w tym snapshocie, bez obietnicy obecnej dostępności mapy.

## Jawne użycie danych eksperymentalnych

Każdy importowany profil ma `status: experimental`, `calibrated: false` i `sourceAcknowledged: false`. Solwer wymaga niezależnej lokalnej kalibracji albo świadomego potwierdzenia źródła dla konkretnego pocisku. Potwierdzenie nie zmienia statusu i nie oznacza sprawdzenia danych w rozgrywce. Nowy import resetuje potwierdzenia w pliku danych.

Zakres elewacji ±89° jest zakresem matematycznego solwera; źródło nie podaje limitów pojazdu. `salvoSize: 1` opisuje obliczenie pojedynczego pocisku, bez twierdzenia o pojemności wyrzutni. Rozrzut (`dispersionSigma`) pozostaje pusty. Grupowanie kategorii aplikacji opiera się na nazwach; główny i pomocniczy system uzbrojenia tego samego pojazdu mogą mieć różne mechaniki.

Źródło nie zawiera modelu oporu powietrza, napędu ani naprowadzania rakiet. Nie tworzymy takich współczynników z nazwy broni. Profile Iskander pozostają w kategorii `guided_missile` z wyłączonym solwerem, nawet po potwierdzeniu źródła. Import nie przenosi pól zasięgu eksplozji ani subamunicji do probabilistycznego rozrzutu; są zachowane w oryginalnym JSON do inspekcji.

## Mapy i wysokość terenu

Katalog `data/maps/community-maps.json` zawiera rozmiar całej mapy (25 map) i, dla 21 map, zakres kodowania heightmapy `heightRangeStuds`. Repozytorium nie zawiera rastrów; `grid.png` jest siatką interfejsu. Tekstury i heightmapy leżą na odrębnym serwerze `maps.artillery-calculator.com` ([utils/images.ts](https://github.com/grand-hawk/artillery-calculator/blob/39a7af7689524e57ca2326f3b998cf123df95426/packages/mtc-artillery/src/utils/images.ts)).

Aplikacja pobiera je **wyłącznie na żądanie, po jednorazowej zgodzie użytkownika** (tryb „Mapa z katalogu” lub panel „Zasoby map”), z dokładnej listy adresów, do lokalnego cache `%LOCALAPPDATA%/MTC-Tactical-Assistant/community-assets-v1` z kontrolą SHA-256. Licencja rastrów pozostaje **nieznana**, dlatego nie dołączamy ich do `.exe`, instalatora ani ZIP. Wysokość z heightmapy (kanał R, `1 stud = 0.36 m gry`) jest używana automatycznie tylko dla tej samej mapy i skali. Brak heightmapy albo próbki oznacza brak wysokości, nie zero. Szczegóły: [zasoby map](community-map-assets.md).

Pełny wymiar mapy dotyczy całej kwadratowej tekstury. Nie stosuj go do powiększonej, obciętej lub obróconej minimapy bez własnej kalibracji. `experienceId` 9520328730 to place wskazany przez źródło (stare MTC4). Aktualna gra używa place 95721658376580; zgodność map z nią nie jest potwierdzona.

## Powtarzalny import

W katalogu głównym, Python 3.10+ bez dodatkowych paczek:

```powershell
python scripts/sync-community-data.py
python scripts/sync-community-data.py --offline
```

Pierwsze polecenie pobiera przypięty commit. Drugie odtwarza dane z dołączonych surowych plików i weryfikuje ich SHA256 oraz commit. Oba polecenia sprawdzono. `--refresh` rozwiązuje aktualny `dev` HEAD; przed wydaniem wymaga przeglądu zmian jednostek, licencji, liczby profili i zgodności z grą. `--commit <pełny-40-znakowy-SHA>` służy do importu konkretnej przejrzanej wersji. Parser traktuje konfigurację jako dane i nie uruchamia JS ani Luau.

Raw źródła: `data/weapons/community/source/` i `data/maps/community/source/`. Manifest zapisuje adres każdego pliku źródłowego, SHA256 i liczbę rekordów. Testy Rust sprawdzają kompletność provenance, brak domyślnej zgody, zgodność jednostek i odtworzenie trajektorii BM-21 w modelu źródła.

## Licencja i dystrybucja

Źródło udostępniono na [licencji MIT](https://github.com/grand-hawk/artillery-calculator/blob/39a7af7689524e57ca2326f3b998cf123df95426/LICENSE.md), `Copyright (c) 2024 Astrid`. Pełny tekst zachowano w `data/weapons/community/LICENSE.md`. Dołącz ten plik do dystrybucji `.exe`/instalatora/ZIP zawierającej dane społeczności. Manifest i ten dokument ułatwiają audyt; nie zastępują wymaganej informacji o licencji. Licencja kodu kalkulatora nie potwierdza licencji oddzielnych zasobów Roblox ani zdalnych obrazów map.
