// @vitest-environment happy-dom
import { describe, it, expect } from 'vitest';
import { sanitizePortName } from './context';

describe('sanitizePortName', () => {
  it('strips leading slash from Unix device paths', () => {
    expect(sanitizePortName('/dev/ttyUSB0')).toBe('dev_ttyUSB0');
  });

  it('replaces dots in device name', () => {
    expect(sanitizePortName('/dev/tty.usbserial-1410')).toBe('dev_tty_usbserial-1410');
  });

  it('leaves Windows COM port names unchanged', () => {
    expect(sanitizePortName('COM3')).toBe('COM3');
  });

  it('replaces backslash', () => {
    expect(sanitizePortName('COM\\3')).toBe('COM_3');
  });

  it('replaces colon', () => {
    expect(sanitizePortName('port:name')).toBe('port_name');
  });

  it('replaces all special chars: * ? " < > |', () => {
    expect(sanitizePortName('a*b?c"d<e>f|g')).toBe('a_b_c_d_e_f_g');
  });

  it('handles empty string', () => {
    expect(sanitizePortName('')).toBe('');
  });
});
