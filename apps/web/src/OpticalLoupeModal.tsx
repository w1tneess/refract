import { useEffect, useRef, useState, type PointerEvent } from 'react';
import type { JobView } from './useJobs.ts';
import { size } from './JobCard.tsx';

interface OpticalLoupeModalProps {
  job: JobView;
  onClose: () => void;
}

export function OpticalLoupeModal({ job, onClose }: OpticalLoupeModalProps) {
  const [sliderPos, setSliderPos] = useState(50); // percentage 0 - 100 for zoom origin
  const [zoom, setZoom] = useState<1 | 2 | 4>(1);

  const containerRef = useRef<HTMLDivElement>(null);
  const clipLayerRef = useRef<HTMLDivElement>(null);
  const dividerRef = useRef<HTMLDivElement>(null);
  const posTextRef = useRef<HTMLSpanElement>(null);

  const isDraggingRef = useRef(false);
  const currentPctRef = useRef(50);
  const rafIdRef = useRef<number | null>(null);

  // Close on Escape key
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [onClose]);

  // Cleanup pending RAF on unmount
  useEffect(() => {
    return () => {
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
      }
    };
  }, []);

  const applyPosition = (pct: number) => {
    currentPctRef.current = pct;
    if (rafIdRef.current !== null) return;

    rafIdRef.current = requestAnimationFrame(() => {
      rafIdRef.current = null;
      const p = currentPctRef.current;
      if (clipLayerRef.current) {
        clipLayerRef.current.style.clipPath = `polygon(0 0, ${p}% 0, ${p}% 100%, 0 100%)`;
      }
      if (dividerRef.current) {
        dividerRef.current.style.left = `${p}%`;
      }
      if (posTextRef.current) {
        posTextRef.current.textContent = `${Math.round(p)}% split`;
      }
    });
  };

  const handlePointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (!containerRef.current) return;
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
    <div
      className="loupe-backdrop"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
      aria-label="Before and after optical loupe"
    >
      <div className="loupe-modal" onClick={(e) => e.stopPropagation()}>
        {/* Header bar */}
        <div className="loupe-header">
          <div className="loupe-title-wrap">
            <span className="loupe-badge">Optical loupe</span>
            <h3 className="loupe-title">
              {job.inputName} <span className="loupe-arrow" aria-hidden="true">→</span> {job.outputName}
            </h3>
          </div>

          <div className="loupe-controls">
            <div className="loupe-zoom-group" role="group" aria-label="Magnification levels">
              {([1, 2, 4] as const).map((z) => (
                <button
                  key={z}
                  type="button"
                  className={`loupe-zoom-btn${zoom === z ? ' is-active' : ''}`}
                  onClick={() => setZoom(z)}
                >
                  {z}x
                </button>
              ))}
            </div>

            <button
              type="button"
              className="loupe-close-btn"
              onClick={onClose}
              aria-label="Close optical loupe"
              title="Close (Esc)"
            >
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <line x1="18" y1="6" x2="6" y2="18" />
                <line x1="6" y1="6" x2="18" y2="18" />
              </svg>
            </button>
          </div>
        </div>

        {/* Comparison Stage */}
        <div
          ref={containerRef}
          className="loupe-stage"
          onPointerDown={handlePointerDown}
          onPointerMove={handlePointerMove}
          onPointerUp={handlePointerUp}
          onPointerCancel={handlePointerUp}
          style={{ touchAction: 'none' }}
        >
          {/* Zoom Wrapper (Hardware-accelerated scale transform) */}
          <div
            className="loupe-zoom-canvas"
            style={{
              transform: `scale(${zoom})`,
              transformOrigin: `${sliderPos}% 50%`,
            }}
          >
            {/* Output image (base layer) */}
            {outputSrc && (
              <img
                src={outputSrc}
                alt="Converted output"
                className="loupe-img loupe-output-img"
                draggable={false}
              />
            )}

            {/* Original image (clipped top layer with direct DOM update) */}
            {originalSrc && (
              <div
                ref={clipLayerRef}
                className="loupe-clip-layer"
                style={{ clipPath: `polygon(0 0, ${sliderPos}% 0, ${sliderPos}% 100%, 0 100%)` }}
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

          {/* Scrubber Divider Line (Hardware-accelerated) */}
          <div
            ref={dividerRef}
            className="loupe-divider"
            style={{ left: `${sliderPos}%` }}
          >
            <div className="loupe-handle" aria-hidden="true">
              <span className="loupe-handle-arrow">‹</span>
              <span className="loupe-handle-diamond">◈</span>
              <span className="loupe-handle-arrow">›</span>
            </div>
          </div>

          {/* Floating Pill Labels */}
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

        {/* Footer Info */}
        <div className="loupe-footer">
          <span className="loupe-hint">
            Drag divider horizontally to inspect compression artifacts and high-frequency edge detail.
          </span>
          <span ref={posTextRef} className="loupe-pos-indicator">
            {sliderPos}% split
          </span>
        </div>
      </div>
    </div>
  );
}
