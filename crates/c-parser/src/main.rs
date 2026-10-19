use c_parser::{parse, ParseInput, ParserConfig};
use std::time::Instant;

fn main() {
    let content = "int main() { 
        int a = 1;
        int b = 2;
        int c = a + b;
        return 0;
    }".repeat(1000); // Создаем большой тестовый файл

    println!("Тест запускается...");
    
    let start = Instant::now();
    let input = ParseInput {
        source: content.as_bytes(),
        filename: Some("test.c"),
        config: ParserConfig::default(),
    };
    
    let result = parse(input).unwrap();
    let json = c_parser::ast_to_json(&result.ast, content.as_bytes()).unwrap();
    
    let duration = start.elapsed();
    println!("Время обработки: {:?}", duration);
    println!("Размер JSON: {} байт", json.len());
    println!("Количество узлов в AST: {}", result.ast.nodes.len());
}
