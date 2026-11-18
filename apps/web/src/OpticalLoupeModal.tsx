import { useEffect, useId, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent } from 'react';
import type { JobView } from './useJobs.ts';
import { size } from './JobCard.tsx';
import { useFocusTrap } from './hooks/useFocusTrap.ts';

interface OpticalLoupeModalProps {
  job: JobView;
  onClose: () => void;
}

export function OpticalLoupeModal({ job, onClose }: OpticalLoupeModalProps) {
  const [sliderPos, setSliderPos] = useState(50);
  const [zoom, setZoom] = useState<1 | 2 | 4>(1);
  const titleId = useId();

  const dialogRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const zoomCanvasRef = useRef<HTMLDivElement>(null);
  const clipLayerRef = useRef<HTMLDivElement>(null);
  const dividerRef = useRef<HTMLDivElement>(null);
  const posTextRef = useRef<HTMLSpanElement>(null);

  const isDraggingRef = useRef(false);
  const currentPctRef = useRef(50);
  const rafIdRef = useRef<number | null>(null);

  useFocusTrap(true, dialogRef, closeRef);

  const applyPosition = (pct: number) => {
    const clamped = Math.max(0, Math.min(100, pct));
    currentPctRef.current = clamped;
    if (rafIdRef.current !== null) return;

    rafIdRef.current = requestAnimationFrame(() => {
      rafIdRef.current = null;
      const p = currentPctRef.current;
      if (zoomCanvasRef.current) {
        zoomCanvasRef.current.style.transformOrigin = `${p}% 50%`;
      }
      if (clipLayerRef.current) {
        clipLayerRef.current.style.clipPath = `inset(0 ${100 - p}% 0 0)`;
      }
      if (dividerRef.current) {
        dividerRef.current.style.left = `${p}%`;
      }
      if (posTextRef.current) {
        posTextRef.current.textContent = `${Math.round(p)}% split`;
      }
    });
  };

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onClose]);

  useEffect(() => {
    return () => {
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
      }
    };
  }, []);

  const handleSliderKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    let next: number | null = null;
    if (e.key === 'ArrowLeft') next = Math.max(0, Math.round(currentPctRef.current) - 5);
    else if (e.key === 'ArrowRight') next = Math.min(100, Math.round(currentPctRef.current) + 5);
    else if (e.key === 'Home') next = 0;
    else if (e.key === 'End') next = 100;

    if (next !== null) {
      e.preventDefault();
      applyPosition(next);
      setSliderPos(next);
    }
  };

  const handlePointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (!containerRef.current || isDraggingRef.current) return;
    isDraggingRef.current = true;
    e.currentTarget.setPointerCapture(e.pointerId);

    const rect = containerRef.current.getBoundingClientRect();
    const x = Math.max(0, Math.min(e.clientX - rect.left, rect.width));
    const pct = Math.max(0, Math.min(100, (x / rect.width) * 100));
    applyPosition(pct);
  };

  const handlePointerMove = (e: PointerEvent<HTMLDivElement>) => {
    if (!isDraggingRef.current || !containerRef.current) return;
    const rect = containerRef.current.getBoundingClientRect();
    const x = Math.max(0, Math.min(e.clientX - rect.left, rect.width));
    const pct = Math.max(0, Math.min(100, (x / rect.width) * 100));
    applyPosition(pct);
  };

  const handlePointerUp = (e: PointerEvent<HTMLDivElement>) => {
    if (isDraggingRef.current) {
      isDraggingRef.current = false;
      try {
        e.currentTarget.releasePointerCapture(e.pointerId);
      } catch {}
      setSliderPos(Math.round(currentPctRef.current));
    }
  };

  const originalSrc = job.originalUrl || job.url;
  const outputSrc = job.url;

  const pctSaved =
    job.inputSize && job.outputSize && job.inputSize > 0
      ? Math.round(((job.outputSize - job.inputSize) / job.inputSize) * 100)
      : null;

  return (
    <div className="loupe-backdrop" onClick={onClose} role="presentation">
      <div
        ref={dialogRef}
        className="loupe-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="loupe-header">
          <div className="loupe-title-wrap">
            <span className="loupe-badge">Compare</span>
            <h2 id={titleId} className="loupe-title">
              {job.inputName} <span className="loupe-arrow" aria-hidden="true">→</span> {job.outputName}
            </h2>
          </div>

          <div className="loupe-controls">
            <div className="loupe-zoom-group" role="group" aria-label="Magnification levels">
              {([1, 2, 4] as const).map((z) => (
                <button
                  key={z}
                  type="button"
                  aria-pressed={zoom === z}
                  className={`loupe-zoom-btn${zoom === z ? ' is-active' : ''}`}
                  onClick={() => setZoom(z)}
                >
                  {z}×
                </button>
              ))}
            </div>

            <button
              ref={closeRef}
              type="button"
              className="loupe-close-btn"
              onClick={onClose}
              aria-label="Close comparison"
              title="Close (Esc)"
            >
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <line x1="18" y1="6" x2="6" y2="18" />
                <line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          </div>
        </div>

        <div
          ref={containerRef}
          className="loupe-stage"
          onPointerDown={handlePointerDown}
          onPointerMove={handlePointerMove}
          onPointerUp={handlePointerUp}
          onPointerCancel={handlePointerUp}
          style={{ touchAction: 'none' }}
        >
          <div
            ref={zoomCanvasRef}
            className="loupe-zoom-canvas"
            style={{
              transform: `scale(${zoom})`,
              transformOrigin: `${sliderPos}% 50%`,
            }}
          >
            {outputSrc && (
              <img
                src={outputSrc}
                alt="Converted output"
                className="loupe-img loupe-output-img"
                draggable={false}
              />
            )}

            {originalSrc && (
              <div
                ref={clipLayerRef}
                className="loupe-clip-layer"
                style={{ clipPath: `inset(0 ${100 - sliderPos}% 0 0)` }}
              >
                <img
                  src={originalSrc}
                  alt="Original input"
                  className="loupe-img loupe-original-img"
                  draggable={false}
                />
              </div>
            )}
          </div>

          <div
            ref={dividerRef}
            className="loupe-divider"
            style={{ left: `${sliderPos}%` }}
          >
            <div
              className="loupe-handle"
              role="slider"
              tabIndex={0}
              aria-label="Before and after split position"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={sliderPos}
              aria-valuetext={`${sliderPos}% original, ${100 - sliderPos}% converted`}
              onKeyDown={handleSliderKeyDown}
            >
              <span className="loupe-handle-arrow" aria-hidden="true">‹</span>
              <span className="loupe-handle-diamond" aria-hidden="true">◈</span>
              <span className="loupe-handle-arrow" aria-hidden="true">›</span>
            </div>
          </div>

          <div className="loupe-pill loupe-pill-left">
            <span className="loupe-pill-tag">Before</span>
            <span className="loupe-pill-size">{size(job.inputSize)}</span>
          </div>

          <div className="loupe-pill loupe-pill-right">
            <span className="loupe-pill-tag">After</span>
            <span className="loupe-pill-size">{size(job.outputSize)}</span>
            {pctSaved !== null && (
              <span className={`loupe-pill-pct ${pctSaved < 0 ? 'is-saved' : 'is-grown'}`}>
                {pctSaved > 0 ? `+${pctSaved}%` : `${pctSaved}%`}
              </span>
            )}
          </div>
        </div>

        <div className="loupe-footer">
          <span className="loupe-hint">
            Drag divider or use arrow keys to compare detail
          </span>
          <span ref={posTextRef} className="loupe-pos-indicator">
            {sliderPos}% split
          </span>
        </div>
      </div>
    </div>
  );
}
