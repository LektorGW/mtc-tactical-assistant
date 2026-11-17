import { describe, expect, it } from 'vitest';
import { catalogScale, clampToImage, pointFromFields, clientToImage, columnLabels, fitView, gridCellSize, localBearing, panView, rowLabels, sectorOf, zoomView } from './map-geometry';
import type { MapGrid } from './types';

const grid: MapGrid = { visible: true, divisions: 9, columns: 'letters-no-i', rowsFrom: 'top' };

describe('skala mapy z katalogu', () => {
  it('liczy pełny wymiar / szerokość tekstury jak kalkulator źródłowy', () => {
    // Normandy Bocage: 664 × 9 studs × 0.36 = 2151.36 m gry na teksturze 1024 px.
    expect(catalogScale(2151.36, 1024, 1024)).toBeCloseTo(2.1009375, 12);
    // |Δ znormalizowane| × wymiar: pół mapy w poziomie = 1075.68 m gry.
    expect(localBearing({ x: 0, y: 512 }, { x: 512, y: 512 }, catalogScale(2151.36, 1024, 1024), 0)!.distance).toBeCloseTo(1075.68, 9);
  });
  it('odrzuca obraz niekwadratowy i brak wymiaru', () => {
    expect(() => catalogScale(1000, 1024, 600)).toThrow(/kwadratowy/);
    expect(() => catalogScale(0, 1024, 1024)).toThrow();
    expect(() => catalogScale(Number.NaN, 1024, 1024)).toThrow();
  });
  it('rozmiar pola siatki 9×9 odpowiada polu z calculateMapSize źródła', () => {
    // calculateMapSize(116) = 116 m × 9 pól → pole = 116 m gry.
    expect(gridCellSize(catalogScale(1044, 1024, 1024), 1024, 9)).toBeCloseTo(116, 9);
  });
});

describe('azymut i dystans', () => {
  it('północ u góry, wschód w prawo, zgodnie z ruchem wskazówek zegara', () => {
    const own = { x: 100, y: 100 };
    for (const [target, azimuth] of [[{ x: 100, y: 50 }, 0], [{ x: 150, y: 100 }, 90], [{ x: 100, y: 150 }, 180], [{ x: 50, y: 100 }, 270], [{ x: 150, y: 50 }, 45]] as const) {
      expect(localBearing(own, target, 2, 0)!.azimuthDeg).toBeCloseTo(azimuth, 9);
    }
    expect(localBearing(own, { x: 150, y: 100 }, 2, 0)!.distance).toBeCloseTo(100, 9);
  });
  it('uwzględnia obrót góry mapy jak Rust MapCalibration', () => {
    // Góra mapy wskazuje wschód: punkt nad stanowiskiem leży na azymucie 90°.
    expect(localBearing({ x: 0, y: 10 }, { x: 0, y: 0 }, 1, 90)!.azimuthDeg).toBeCloseTo(90, 9);
  });
  it('nie zwraca azymutu dla identycznych punktów ani bez skali', () => {
    expect(localBearing({ x: 1, y: 1 }, { x: 1, y: 1 }, 1, 0)).toBeNull();
    expect(localBearing({ x: 1, y: 1 }, { x: 2, y: 1 }, 0, 0)).toBeNull();
  });
});

describe('siatka sektorów', () => {
  it('kolumny literowe bez I, wiersze liczbowe od góry', () => {
    expect(columnLabels(9, 'letters-no-i').join('')).toBe('ABCDEFGHJ');
    expect(columnLabels(9, 'letters')).toContain('I');
    expect(columnLabels(25, 'letters-no-i')).not.toContain('I');
    expect(columnLabels(3, 'numbers')).toEqual(['1', '2', '3']);
    expect(rowLabels(3, 'top')).toEqual(['1', '2', '3']);
    expect(rowLabels(3, 'bottom')).toEqual(['3', '2', '1']);
  });
  it('wyznacza sektor punktu łącznie z ostatnią krawędzią mapy', () => {
    expect(sectorOf({ x: 0, y: 0 }, 900, 900, grid)!.label).toBe('A1');
    expect(sectorOf({ x: 850, y: 120 }, 900, 900, grid)!.label).toBe('J2');
    expect(sectorOf({ x: 900, y: 900 }, 900, 900, grid)!.label).toBe('J9');
    expect(sectorOf({ x: 450, y: 450 }, 900, 900, { ...grid, rowsFrom: 'bottom' })!.label).toBe('E5');
    expect(sectorOf({ x: -1, y: 0 }, 900, 900, grid)).toBeNull();
  });
});

describe('klik → piksel mapy przy zoomie i przesunięciu', () => {
  const box = { left: 40, top: 100, width: 500, height: 500 };
  it('używa naturalnego rozmiaru obrazu, nie wyświetlanego', () => {
    // Obraz 1024 px wyświetlony w 500 px CSS (np. DPI 150%): środek elementu = środek obrazu.
    expect(clientToImage(290, 350, box, fitView(1024, 1024))).toEqual({ x: 512, y: 512 });
  });
  it('po zoomie wokół kursora punkt pod kursorem się nie przesuwa', () => {
    const anchor = clientToImage(140, 200, box, fitView(1024, 1024));
    const zoomed = zoomView(fitView(1024, 1024), 4, anchor, 1024, 1024);
    expect(zoomed.width).toBeCloseTo(256, 9);
    const after = clientToImage(140, 200, box, zoomed);
    expect(after.x).toBeCloseTo(anchor.x, 9);
    expect(after.y).toBeCloseTo(anchor.y, 9);
  });
  it('przesunięcie jest ograniczone do obrazu', () => {
    const zoomed = zoomView(fitView(1000, 1000), 2, { x: 500, y: 500 }, 1000, 1000);
    const panned = panView(zoomed, 10_000, -10_000, 1000, 1000);
    expect(panned).toEqual({ x: 500, y: 0, width: 500, height: 500 });
    expect(clientToImage(540, 600, box, panned)).toEqual({ x: 1000, y: 500 });
  });
  it('uwzględnia pasy letterbox przy innych proporcjach elementu', () => {
    const wide = { left: 0, top: 0, width: 800, height: 400 };
    // Obraz 400×400 w elemencie 800×400: pas 200 px po lewej.
    expect(clientToImage(200, 0, wide, fitView(400, 400))).toEqual({ x: 0, y: 0 });
  });
  it('zoom nie przekracza limitu i nie oddala poza całą mapę', () => {
    expect(zoomView(fitView(1024, 1024), 1000, { x: 0, y: 0 }, 1024, 1024).width).toBeCloseTo(64, 9);
    expect(zoomView(fitView(1024, 1024), 0.1, { x: 0, y: 0 }, 1024, 1024)).toEqual(fitView(1024, 1024));
  });
  it('punkt kliknięcia mieści się w zakresie heightmap 0..rozmiar−1', () => {
    expect(clampToImage({ x: 1024, y: -3 }, 1024, 1024)).toEqual({ x: 1023, y: 0 });
  });
});

describe('pola współrzędnych', () => {
  it('tworzy punkt dopiero przy obu współrzędnych w zakresie obrazu', () => {
    expect(pointFromFields(10, null, 100, 100)).toBe('incomplete');
    expect(pointFromFields(null, 5, 100, 100)).toBe('incomplete');
    expect(pointFromFields(101, 5, 100, 100)).toBe('incomplete');
    expect(pointFromFields(null, null, 100, 100)).toBeNull();
    expect(pointFromFields(10, 0, 100, 100)).toEqual({ x: 10, y: 0 });
  });
  it('domyślna siatka zgodna ze zrzutem gry: A–H, J i wiersze od góry (np. C7)', () => {
    const p = { x: 2.5 * 100, y: 6.5 * 100 };
    expect(sectorOf(p, 900, 900, { visible: true, divisions: 9, columns: 'letters-no-i', rowsFrom: 'top' })!.label).toBe('C7');
  });
});
