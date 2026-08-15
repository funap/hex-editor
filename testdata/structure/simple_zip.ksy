meta:
  id: simple_zip
  endian: le
seq:
  - id: sections
    type: section
    repeat: eos
types:
  section:
    seq:
      - id: magic
        size: 2
      - id: section_type
        type: u2
      - id: body
        size: 26
