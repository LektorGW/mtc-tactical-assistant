# Automatyczna lokalizacja jawnych danych HUD

`mtc-vision` odczytuje cały obraz systemowym OCR Windows i zwraca położenia rozpoznanych słów. Na podstawie formatu, etykiet i geometrii proponuje małe obszary do kolejnych odczytów. Nie rozpoznaje pojazdu po sylwetce, nie przypisuje parametrów broni i nie wylicza współrzędnych świata z dowolnych liczb.

## Publiczne API

- `ocr_layout(image) -> Result<OcrLayout, String>`: tekst, wiersze, słowa i ich normalizowane ROI.
- `discover_hud(image, timestamp_ms, now_ms) -> Result<HudDiscovery, String>`: pełny OCR oraz propozycje ROI ze wskaźnikiem heurystycznej pewności i dowodami.
- `discovery_from_layout(layout, timestamp_ms, now_ms) -> HudDiscovery`: czysta funkcja używana w deterministycznych regresjach.
- `normalize_ocr_box(...)`: przeliczenie Windows `BoundingRect`, wraz z obrotem tekstu, do oryginalnego obrazu.
- `analyze_frame_from_discovery(image, timestamp_ms, now_ms, config, discovery) -> Result<FrameAnalysis, String>`: pomiary z tekstu już rozpoznanego w tej samej klatce dla zaakceptowanych automatycznych ROI, wraz z bieżącym CV markerów.

Dotychczasowe `ocr` pozostaje dostępne i zwraca sam tekst tego samego systemowego odczytu. Pozostałe API biblioteki nie zmieniły formatu.

`OcrLayout` jest serializowany w `camelCase`:

```text
schemaVersion: 1
sourceWidth, sourceHeight: wymiary oryginalnej klatki
recognizedWidth, recognizedHeight: wymiary obrazu przekazanego do silnika OCR
text, textAngleDeg, source, elapsedMs
lines: [{ text, roi, words: [{ text, roi }] }]
```

`roi` zawsze odnosi się do oryginalnej klatki i ma wartości `x`, `y`, `width`, `height` w zakresie 0–1. Silnik może zmniejszyć duży obraz do `MaxImageDimension`; proporcje zostają zachowane. Przy `TextAngle` różnym od zera biblioteka obraca cztery rogi każdego prostokąta wokół środka obrazu, następnie bierze ich obwiednię i normalizuje ją. Prostokąty słów są źródłem geometrii wiersza. Słowa całkowicie poza oryginalną klatką nie uczestniczą w discovery.

Format `HudDiscovery`:

```text
schemaVersion: 1
status: ready | partial | ambiguous | blocked
timestampMs, referenceWidth, referenceHeight, elapsedMs, source
regions: [{
  region: { id, kind, roi, upscale },
  confidence,
  text,
  value: number | null,
  evidence: string[]
}]
warnings: string[]
```

`region` jest istniejącym `OcrRegionConfig` i można go przekazać do `AnalysisConfig.ocrRegions`. Nazwy identyfikatorów są stabilne, np. `auto_timer`, `auto_heading`, `auto_vehicleName`. `referenceWidth` i `referenceHeight` należy zachować jako referencję geometrii przechwytywania. Discovery zwraca co najwyżej 16 propozycji; obecnie rozróżnia siedem typów danych.

## Wymagane dowody

**Czas meczu:** dokładnie jedna poprawna wartość `MM:SS` w krótkim wierszu centralnej górnej części obrazu albo przy jawnej etykiecie czasu. Kilka czasów, sekundy poza zakresem, dłuższy wiersz czatu lub zegar w dolnej części obrazu nie stają się czasem meczu. OCR może rozdzielić minuty i sekundy na dwa słowa; geometryczny wiersz pozwala je złożyć.

**Kompas:** jawna etykieta `Heading`, `Azimuth`, `Bearing`, `HDG`, `Kompas`, `Kierunek` albo wyróżniona liczba centralna przy widocznej skali. W drugim przypadku odczyt musi być co najmniej 1,35 razy wyższy od sąsiednich oznaczeń, znajdować się między nimi, w górnej części HUD i przy osi centralnej. Jego wartość musi leżeć w widocznym łuku skali, również przy przejściu 359°/0°. Równorzędne oznaczenia skali nie są odczytem bieżącego kierunku. Liczby przypisane do innego jawnie oznaczonego pola nie przechodzą przez ten fallback.

**Prędkość i wysokość:** wyłącznie po etykietach, np. `Speed`, `Velocity`, `Altitude`, `Height`, `Prędkość`, `Wysokość`. Jednostki widoczne w tekście są zachowane jako dowód; discovery nie potwierdza ich zgodności z jednostkami mapy ani profilu. Brak etykiety oznacza brak propozycji. Skrót `Alt` na fotografii klawiatury nie jest telemetrią. Bliski osobny wiersz wartości można przypisać do etykiety tylko przy jednoznacznej geometrii.

**Nazwy:** `Vehicle`/`Pojazd`, `Weapon`/`Broń`/`Uzbrojenie`, `Ammo`/`Ammunition`/`Projectile`/`Amunicja` oraz `Map`/`Mapa`. `Ammo` jest propozycją typu `weaponName` z zachowanym dowodem etykiety; późniejszy resolver powinien odróżnić amunicję od nazwy platformy. Licznik amunicji `12/40` nie daje nazwy. Wiersze wyboru (`Choose`, `Select`, `Wybierz`, `Menu`, `Spawn`) są pomijane.

Proponowany ROI nazwy obejmuje słowa jej wartości. Jeżeli OCR złączy etykietę i nazwę w jeden prostokąt, np. `Vehicle:BM21`, biblioteka nie zgaduje szerokości znaków: pozostaje ręczne ROI i ostrzeżenie. Wartość `BM-21 Grad` na obrazie bez jawnej etykiety nie potwierdza aktywnego pojazdu. Nawet poprawna nazwa nie potwierdza mechaniki ani parametrów pocisku.

## Świeżość i zastosowanie

Klatka starsza niż 2000 ms lub z przyszłym czasem jest blokowana. `discover_hud` sprawdza czas również po pełnym OCR, uwzględniając czas przetwarzania. Geometria źródła musi być poprawna, co najmniej 160 × 90 px, a silny obrót tekstu ponad 10° blokuje propozycje. OCR nie może zmienić proporcji źródła. Odczyt ma limity rozmiaru obrazu, wierszy, słów i tekstu.

`ready` oznacza co najmniej dwa jednoznacznie znalezione typy, `partial` jeden typ, `ambiguous` konkurencyjne obszary lub wartości dla tego samego typu, `blocked` brak dowodów albo niespełnione warunki. Stan `ready` nie oznacza kompletu danych balistycznych. Niejednoznaczny typ nie dostaje wybranej arbitralnie propozycji; inne jednoznaczne typy mogą pozostać w wyniku.

Pojedyncza klatka otrzymuje najwyżej 78% heurystycznej pewności. Automatyczne zastosowanie wymaga co najmniej dwóch świeżych zgodnych odkryć tego samego typu i zachodzących na siebie ROI, następnie stabilnych właściwych obserwacji z tych ROI. Próg co najmniej 85% powinien wymusić konsument. Zmiana okna, geometrii lub układu HUD zeruje historię. Typy, których nie udało się znaleźć, wymagają stanu `brak danych`, bez zastępowania przypadkowymi liczbami.

Przy pełnym OCR każdej klatki `analyze_frame_from_discovery` ponownie wykorzystuje rozpoznany tekst i nie wykonuje drugiego OCR ciasnych wycinków. Źródło discovery musi mieć dokładnie ten sam czas i rozmiary klatki. Powiązanie jest po ID, typie i zaakceptowanym ROI; kolejność propozycji nie ma znaczenia. Każdy zaakceptowany obszar wymaga co najmniej 85% potwierdzonej heurystyki lokalizacji oraz dowodu. Pola niewykryte, przesunięte lub niepotwierdzone pozostają zablokowane. Liczby są ponownie parsowane z tekstu; `proposal.value` nie zastępuje odczytu. Pewność położenia nie podnosi automatycznie pewności wartości: `stabilize_frame` wymaga dwóch różnych, świeżych znaczników czasu. Format JSON wyników pozostaje taki sam.

Pełny OCR może również służyć lokalizacji obszarów wykonywanej rzadziej niż późniejsze małe odczyty ręcznym API. Nie wolno przekazać historycznej discovery jako odczytu nowej klatki. Biblioteka nie zapisuje ani nie wysyła obrazów. Systemowe OCR może być niedostępne przy braku odpowiednich języków lub ograniczeniach Windows; błąd jest zwracany jawnie.

## Testy

Regresje deterministyczne obejmują format czasu, czat i konkurencyjne timery, wyróżniony kompas i przejście przez północ, odrzucanie zwykłych liczb, jawne etykiety PL/EN, czyste ROI nazw, brak nazw z sylwetki lub menu, niejednoznaczne wartości przy etykiecie, niewłaściwą geometrię i przeterminowanie. Weryfikacja transformacji osobno sprawdza skalowanie oraz obrót zgodny z Windows `TextAngle`.

Rzeczywisty `ocr_layout` na obu dostarczonych fotografiach znalazł czasy `27:42` i `29:33`. Wykryte słowa miały poprawne prostokąty źródłowe; niezależny OCR z automatycznie zaproponowanych wycinków ponownie odczytał te czasy. Zdjęcia nie dostarczają dowodu nazw pojazdu, broni ani mapy. Pełny OCR nie dostarczył wystarczającego dowodu wyróżnionej skali kompasu, więc discovery nie przypisało kierunku. Pola testowane syntetycznie nie są przedstawiane jako potwierdzone na aktualnym HUD MTC.

Polecenia: `cargo test -p mtc-vision`, `cargo clippy -p mtc-vision --all-targets -- -D warnings`, `cargo fmt -p mtc-vision -- --check`.

## Dokumentacja źródłowa

Położenia słów pochodzą z [`OcrWord.BoundingRect`](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr.ocrword.boundingrect), a słowa są grupowane przez [`OcrLine.Words`](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr.ocrline.words). Zasady obracania współrzędnych określa [`OcrResult.TextAngle`](https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr.ocrresult.textangle). Implementacja discovery jest własna; nie skopiowano kodu z zewnętrznego repozytorium.
