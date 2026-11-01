import Foundation

/// A JSON value. Events, setup objects and API replies are open-ended (each event type has its own
/// `data`), so the app keeps them as values and reads fields by name.
enum JSON: Hashable {
    case null
    case bool(Bool)
    case number(Double)
    case string(String)
    case array([JSON])
    case object([String: JSON])

    // MARK: Parsing and encoding

    /// Parses with JSONSerialization: event logs are large and this is much faster than Codable.
    static func parse(_ data: Data) throws -> JSON {
        if data.isEmpty { return .object([:]) }
        return JSON(any: try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed]))
    }

    init(any value: Any?) {
        switch value {
        case nil, is NSNull: self = .null
        case let n as NSNumber:
            self = CFGetTypeID(n) == CFBooleanGetTypeID() ? .bool(n.boolValue) : .number(n.doubleValue)
        case let s as String: self = .string(s)
        case let a as [Any]: self = .array(a.map { JSON(any: $0) })
        case let o as [String: Any]: self = .object(o.mapValues { JSON(any: $0) })
        default: self = .null
        }
    }

    var any: Any {
        switch self {
        case .null: return NSNull()
        case .bool(let b): return b
        case .number(let n): return n.rounded() == n && abs(n) < 9e15 ? Int64(n) as Any : n
        case .string(let s): return s
        case .array(let a): return a.map(\.any)
        case .object(let o): return o.mapValues(\.any)
        }
    }

    func data(pretty: Bool = false) -> Data {
        let opts: JSONSerialization.WritingOptions = pretty ? [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes, .fragmentsAllowed] : [.fragmentsAllowed, .withoutEscapingSlashes]
        return (try? JSONSerialization.data(withJSONObject: any, options: opts)) ?? Data("null".utf8)
    }

    func pretty() -> String { String(decoding: data(pretty: true), as: UTF8.self) }

    // MARK: Access

    subscript(key: String) -> JSON {
        get { if case .object(let o) = self { return o[key] ?? .null }; return .null }
        set {
            var o: [String: JSON]
            if case .object(let cur) = self { o = cur } else { o = [:] }
            o[key] = newValue
            self = .object(o)
        }
    }

    subscript(index: Int) -> JSON {
        if case .array(let a) = self, a.indices.contains(index) { return a[index] }
        return .null
    }

    var string: String? { if case .string(let s) = self { return s }; return nil }
    /// The value as display text, "" for null (strings as they are, numbers without ".0").
    var str: String {
        switch self {
        case .string(let s): return s
        case .number(let n): return n.rounded() == n && abs(n) < 9e15 ? String(Int64(n)) : String(n)
        case .bool(let b): return b ? "true" : "false"
        case .null: return ""
        default: return String(decoding: data(), as: UTF8.self)
        }
    }
    var double: Double? { if case .number(let n) = self { return n }; return nil }
    var int: Int? { double.map { Int($0) } }
    var bool: Bool? { if case .bool(let b) = self { return b }; return nil }
    var array: [JSON] { if case .array(let a) = self { return a }; return [] }
    var object: [String: JSON] { if case .object(let o) = self { return o }; return [:] }
    var strings: [String] { array.map(\.str) }
    var isNull: Bool { self == .null }
    var exists: Bool { self != .null }

    /// JavaScript-style truthiness: a field counts when it is present and not empty, zero or false.
    var truthy: Bool {
        switch self {
        case .null: return false
        case .bool(let b): return b
        case .number(let n): return n != 0 && !n.isNaN
        case .string(let s): return !s.isEmpty
        default: return true
        }
    }

    /// `self || other` in JavaScript terms.
    func or(_ other: JSON) -> JSON { truthy ? self : other }
}

extension JSON: ExpressibleByStringLiteral, ExpressibleByIntegerLiteral, ExpressibleByBooleanLiteral, ExpressibleByArrayLiteral, ExpressibleByDictionaryLiteral, ExpressibleByNilLiteral, ExpressibleByFloatLiteral {
    init(stringLiteral v: String) { self = .string(v) }
    init(integerLiteral v: Int) { self = .number(Double(v)) }
    init(floatLiteral v: Double) { self = .number(v) }
    init(booleanLiteral v: Bool) { self = .bool(v) }
    init(arrayLiteral v: JSON...) { self = .array(v) }
    init(dictionaryLiteral v: (String, JSON)...) { self = .object(Dictionary(v, uniquingKeysWith: { $1 })) }
    init(nilLiteral: ()) { self = .null }
}

extension JSON {
    init(_ s: String?) { self = s.map { .string($0) } ?? .null }
    init(_ n: Int) { self = .number(Double(n)) }
    init(_ b: Bool) { self = .bool(b) }
    init(_ xs: [String]) { self = .array(xs.map { .string($0) }) }
}
