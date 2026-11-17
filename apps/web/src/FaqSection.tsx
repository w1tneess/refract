import { useState } from 'react';

type FaqItem = {
  question: string;
  answer: string;
};

const FAQS: FaqItem[] = [
  {
    question: 'How does Refract convert files without uploading them?',
    answer:
      'Refract uses WebAssembly (WASM) — a high-performance binary format that allows native C, C++, and Rust code (such as MozJPEG, LibWebP, QPDF, and FFmpeg) to run directly inside your browser sandbox. The browser reads files from your disk into local memory, processes them using Web Workers on your CPU, and writes the output back to your computer. No network requests are made.',
  },
  {
    question: 'Are my confidential documents or private photos safe?',
    answer:
      'Yes, 100%. Because there is no backend server receiving your files, your documents, tax returns, contracts, and personal photos literally cannot be intercepted, logged, or saved by third parties. You can even disconnect your Wi-Fi or turn on Airplane mode and Refract will continue working smoothly.',
  },
  {
    question: 'What is the maximum file size I can process?',
    answer:
      'Unlike cloud converters that throttle you at 10 MB or 25 MB unless you pay, Refract has no artificial paywalls or file caps. The limit is determined solely by your computer’s available RAM. For high-resolution photos and multi-page PDFs, Refract handles files up to several hundred megabytes with ease.',
  },
  {
    question: 'How does encrypted PDF unlocking work?',
    answer:
      'When you supply a password-protected PDF, Refract prompts you for the password right in the job card. The decryption algorithm runs locally via QPDF WASM. You can also check "Use for all locked files" to unlock multiple batch PDFs with one click.',
  },
  {
    question: 'Is Refract free and open source?',
    answer:
      'Yes. Refract is licensed under GPL-3.0-or-later. The entire source code is openly auditable, with no telemetry, no tracking cookies, no accounts, and no advertisements.',
  },
];

export function FaqSection() {
  const [openIdx, setOpenIdx] = useState<number | null>(null);

  const toggle = (idx: number) => {
    setOpenIdx(openIdx === idx ? null : idx);
  };

  return (
    <section className="faq-section" aria-label="Frequently asked questions">
      <div className="section-head">
        <span className="section-tag">COMMON INQUIRIES</span>
        <h2 className="section-title">Frequently Asked Questions</h2>
        <p className="section-subtitle">
          Everything you need to know about local-first WebAssembly file transformation.
        </p>
      </div>

      <div className="faq-list">
        {FAQS.map((faq, idx) => {
          const isOpen = openIdx === idx;
          const id = `faq-${idx}`;
          return (
            <div key={faq.question} className={`faq-item${isOpen ? ' is-open' : ''}`}>
              <button
                type="button"
                className="faq-question"
                onClick={() => toggle(idx)}
                aria-expanded={isOpen}
                aria-controls={`${id}-answer`}
              >
                <span>{faq.question}</span>
                <span className="faq-chevron" aria-hidden="true">
                  {isOpen ? '−' : '+'}
                </span>
              </button>
              {isOpen && (
                <div id={`${id}-answer`} className="faq-answer">
                  <p>{faq.answer}</p>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </section>
  );
}
