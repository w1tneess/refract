import type { Options, ToolId } from '@refract/core';

export function UseCasesSection({
  onSelectTool,
  onApplyWorkflow,
}: {
  onSelectTool: (tool: ToolId) => void;
  onApplyWorkflow: (tool: ToolId, options?: Options) => void;
}) {
  return (
    <section className="editorial-section" aria-label="Why Refract and Architecture">
      {/* 3-Pillar Clean Editorial Principles */}
      <div className="pillars-grid">
        <div className="pillar-item">
          <span className="pillar-num">01</span>
          <h3 className="pillar-title">On-device processing</h3>
          <p className="pillar-desc">
            Files never touch a remote server. Everything is decoded, transformed, and written in RAM via WebAssembly Web Workers.
          </p>
          <span className="pillar-meta">Zero network telemetry</span>
        </div>

        <div className="pillar-item">
          <span className="pillar-num">02</span>
          <h3 className="pillar-title">Native performance</h3>
          <p className="pillar-desc">
            Compiled from industry-standard libraries including MozJPEG, LibWebP, QPDF, and FFmpeg running directly on your CPU.
          </p>
          <span className="pillar-meta">WebAssembly pipeline</span>
        </div>

        <div className="pillar-item">
          <span className="pillar-num">03</span>
          <h3 className="pillar-title">No arbitrary limits</h3>
          <p className="pillar-desc">
            No file size paywalls, no daily conversion counters, no email capture, and no subscriptions.
          </p>
          <span className="pillar-meta">GPL-3.0 license</span>
        </div>
      </div>

      {/* Curated Workflows Grid */}
      <div className="workflows-container">
        <div className="section-meta-header">
          <span className="meta-tag">Capabilities</span>
          <h2 className="editorial-heading">Built for high-stakes workflows</h2>
        </div>

        <div className="workflows-grid">
          {/* Workflow 1 */}
          <div className="workflow-card">
            <span className="workflow-badge">Confidential documents</span>
            <h4 className="workflow-title">NDAs, contracts, and tax documents</h4>
            <p className="workflow-text">
              Merge, split, and reorder sensitive corporate PDFs without third-party exposure. Includes AES password unlock and lossless QPDF stream compression.
            </p>
            <button
              type="button"
              className="editorial-link-btn"
              onClick={() => onApplyWorkflow('merge-pdf')}
            >
              Open PDF merge
            </button>
          </div>

          {/* Workflow 2 */}
          <div className="workflow-card">
            <span className="workflow-badge">Photography</span>
            <h4 className="workflow-title">iPhone HEIC and camera photos</h4>
            <p className="workflow-text">
              Batch-convert Apple HEIC photos into WebP or JPEG with automatic EXIF orientation baking and Lanczos-3 resampling.
            </p>
            <button
              type="button"
              className="editorial-link-btn"
              onClick={() => onApplyWorkflow('convert-image', { to: 'jpeg', quality: 92 })}
            >
              Convert HEIC to JPEG
            </button>
          </div>

          {/* Workflow 3 */}
          <div className="workflow-card">
            <span className="workflow-badge">Audio</span>
            <h4 className="workflow-title">Audio snippets and voice memos</h4>
            <p className="workflow-text">
              Transcode between MP3, WAV, M4A, and OGG with custom bitrates. Cut audio clips with millisecond timestamp precision.
            </p>
            <button
              type="button"
              className="editorial-link-btn"
              onClick={() => onSelectTool('trim-audio')}
            >
              Trim audio track
            </button>
          </div>

          {/* Workflow 4 */}
          <div className="workflow-card">
            <span className="workflow-badge">Web performance</span>
            <h4 className="workflow-title">Target-size compression</h4>
            <p className="workflow-text">
              Compress images down to strict byte budgets with binary quality search to improve page load times and Core Web Vitals.
            </p>
            <button
              type="button"
              className="editorial-link-btn"
              onClick={() => onApplyWorkflow('compress-image', { targetKB: 100 })}
            >
              Target-size compression
            </button>
          </div>
        </div>
      </div>

      {/* Comparison: Refract vs Cloud Converters */}
      <div className="benchmark-card">
        <div className="section-meta-header">
          <span className="meta-tag">Technical benchmark</span>
          <h3 className="editorial-subheading">Refract vs cloud file converters</h3>
        </div>

        <div className="table-responsive">
          <table className="benchmark-table">
            <thead>
              <tr>
                <th scope="col">Dimension</th>
                <th scope="col" className="col-refract">Refract (local WebAssembly)</th>
                <th scope="col" className="col-cloud">Typical cloud converters</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td><strong>Data privacy</strong></td>
                <td className="col-refract">
                  <span className="bullet-check">✓</span> <strong>Local memory only.</strong> Files never leave your machine.
                </td>
                <td className="col-cloud">
                  <span className="bullet-cross">✗</span> Uploaded to remote servers across the network.
                </td>
              </tr>
              <tr>
                <td><strong>Latency and speed</strong></td>
                <td className="col-refract">
                  <span className="bullet-check">✓</span> <strong>Instant CPU execution.</strong> No upload or download delay.
                </td>
                <td className="col-cloud">
                  <span className="bullet-cross">✗</span> Dependent on upload bandwidth, server queue, and download time.
                </td>
              </tr>
              <tr>
                <td><strong>Usage quotas</strong></td>
                <td className="col-refract">
                  <span className="bullet-check">✓</span> <strong>Unlimited.</strong> No fees, no daily limits, no paywalls.
                </td>
                <td className="col-cloud">
                  <span className="bullet-cross">✗</span> Throttled after small quotas; requires monthly subscription.
                </td>
              </tr>
              <tr>
                <td><strong>Offline mode</strong></td>
                <td className="col-refract">
                  <span className="bullet-check">✓</span> <strong>Works offline.</strong> Convert on flights, transit, or disconnected.
                </td>
                <td className="col-cloud">
                  <span className="bullet-cross">✗</span> Requires an active internet connection.
                </td>
              </tr>
              <tr>
                <td><strong>Auditability</strong></td>
                <td className="col-refract">
                  <span className="bullet-check">✓</span> <strong>GPL-3.0 open source.</strong> Open and verifiable code.
                </td>
                <td className="col-cloud">
                  <span className="bullet-cross">✗</span> Proprietary closed systems with tracking scripts.
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </section>
  );
}
