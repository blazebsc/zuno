import { useEffect, useRef } from "react";
import { SPECTRUM_BANDS, subscribeSpectrum } from "../audio/demoPlayer";

const BARS = SPECTRUM_BANDS * 2;

/** A ring of bars around the record, mirrored so bass sits at the top. Idle, it is a faint dial. */
export function Spectrum({ className }: { className?: string }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const context = canvas?.getContext("2d");
    if (!canvas || !context) return;

    const styles = getComputedStyle(canvas);
    const from = styles.getPropertyValue("--color-primary").trim();
    const to = styles.getPropertyValue("--color-headphone").trim();
    let size = 0;
    let stroke: CanvasGradient | string = from;
    let last: Float32Array | null = null;

    const draw = (levels: Float32Array | null) => {
      last = levels;
      context.clearRect(0, 0, size, size);
      const centre = size / 2;
      const inner = size * 0.445;
      const reach = size * 0.05;
      context.lineCap = "round";
      context.lineWidth = Math.max(1.5, size * 0.0065);
      context.strokeStyle = stroke;
      for (let bar = 0; bar < BARS; bar += 1) {
        const band = bar < SPECTRUM_BANDS ? bar : BARS - 1 - bar;
        const level = levels ? levels[band] : 0;
        const angle = (bar / BARS) * Math.PI * 2 - Math.PI / 2;
        const length = size * 0.008 + level * reach;
        const cos = Math.cos(angle);
        const sin = Math.sin(angle);
        context.globalAlpha = 0.22 + level * 0.78;
        context.beginPath();
        context.moveTo(centre + cos * inner, centre + sin * inner);
        context.lineTo(centre + cos * (inner + length), centre + sin * (inner + length));
        context.stroke();
      }
      context.globalAlpha = 1;
    };

    const resize = () => {
      const box = canvas.getBoundingClientRect();
      const ratio = Math.min(2, window.devicePixelRatio || 1);
      size = box.width;
      canvas.width = Math.round(box.width * ratio);
      canvas.height = Math.round(box.height * ratio);
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      const gradient = context.createConicGradient(-Math.PI / 2, size / 2, size / 2);
      gradient.addColorStop(0, from);
      gradient.addColorStop(0.5, to);
      gradient.addColorStop(1, from);
      stroke = gradient;
      draw(last);
    };

    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(canvas);
    const unsubscribe = subscribeSpectrum(draw);
    return () => {
      observer.disconnect();
      unsubscribe();
    };
  }, []);

  return <canvas ref={canvasRef} className={className} aria-hidden="true" />;
}
