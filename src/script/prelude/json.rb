# Internals: JSON for handing the model and actions to Rust. This mruby
# build has no JSON library, and the prelude needs only encoding.

module Knut
  module Json
    class << self
      def encode(value)
        case value
        when Hash then object(value)
        when Array then "[" + value.map { |item| encode(item) }.join(",") + "]"
        when String then string(value)
        when Symbol then string(value.to_s)
        when Integer then value.to_s
        when Float then value.finite? ? value.to_s : "null"
        when true, false then value.to_s
        when nil then "null"
        when Time then value.to_i.to_s
        else string(value.to_s)
        end
      end

      private

      def object(hash)
        members = hash.map { |key, item| "#{string(key.to_s)}:#{encode(item)}" }
        "{" + members.join(",") + "}"
      end

      def string(text)
        escaped = text.each_char.map { |char| escape(char) }
        "\"#{escaped.join}\""
      end

      def escape(char)
        case char
        when '"' then '\\"'
        when "\\" then "\\\\"
        when "\n" then "\\n"
        when "\r" then "\\r"
        when "\t" then "\\t"
        else char.ord < 0x20 ? format("\\u%04x", char.ord) : char
        end
      end
    end
  end
end
