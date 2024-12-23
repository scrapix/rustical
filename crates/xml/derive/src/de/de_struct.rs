use crate::de::attrs::StructAttrs;
use crate::de::Field;
use core::panic;
use darling::FromDeriveInput;
use quote::quote;
use syn::{DataStruct, DeriveInput};

fn invalid_field_branch(allow: bool) -> proc_macro2::TokenStream {
    if allow {
        quote! {}
    } else {
        quote! { return Err(XmlDeError::InvalidFieldName) }
    }
}

impl NamedStruct {
    pub fn parse(input: &DeriveInput, data: &DataStruct) -> Self {
        let attrs = StructAttrs::from_derive_input(input).unwrap();

        match &data.fields {
            syn::Fields::Named(named) => NamedStruct {
                fields: named
                    .named
                    .iter()
                    .map(|field| Field::from_syn_field(field.to_owned(), attrs.container.clone()))
                    .collect(),
                attrs,
                ident: input.ident.to_owned(),
                generics: input.generics.to_owned(),
            },
            syn::Fields::Unnamed(_) => panic!("not implemented for tuple struct"),
            syn::Fields::Unit => NamedStruct {
                fields: vec![],
                attrs,
                ident: input.ident.to_owned(),
                generics: input.generics.to_owned(),
            },
        }
    }
}

pub struct NamedStruct {
    attrs: StructAttrs,
    fields: Vec<Field>,
    ident: syn::Ident,
    generics: syn::Generics,
}

impl NamedStruct {
    pub fn impl_xml_root(&self) -> proc_macro2::TokenStream {
        let (impl_generics, type_generics, where_clause) = self.generics.split_for_impl();
        let ident = &self.ident;
        let root = self.attrs.root.as_ref().expect("No root attribute found");
        let ns = match &self.attrs.ns {
            Some(ns) => quote! { Some(#ns) },
            None => quote! { None },
        };
        quote! {
            impl #impl_generics ::rustical_xml::XmlRoot for #ident #type_generics #where_clause {
                fn root_tag() -> &'static [u8] { #root }
                fn root_ns() -> Option<&'static [u8]> { #ns }
            }
        }
    }

    pub fn impl_de(&self) -> proc_macro2::TokenStream {
        let (impl_generics, type_generics, where_clause) = self.generics.split_for_impl();
        let ident = &self.ident;

        let builder_fields = self.fields.iter().map(Field::builder_field);
        let builder_field_inits = self.fields.iter().map(Field::builder_field_init);
        let named_field_branches = self.fields.iter().filter_map(Field::named_branch);
        let untagged_field_branches: Vec<_> = self
            .fields
            .iter()
            .filter_map(Field::untagged_branch)
            .collect();
        if untagged_field_branches.len() > 1 {
            panic!("Currently only one untagged field supported!");
        }
        let text_field_branches = self.fields.iter().filter_map(Field::text_branch);
        let attr_field_branches = self.fields.iter().filter_map(Field::attr_branch);

        let builder_field_builds = self.fields.iter().map(Field::builder_field_build);

        let invalid_field_branch = invalid_field_branch(self.attrs.allow_invalid.is_present());

        quote! {
            impl #impl_generics ::rustical_xml::XmlDeserialize for #ident #type_generics #where_clause {
                fn deserialize<R: ::std::io::BufRead>(
                    reader: &mut quick_xml::NsReader<R>,
                    start: &quick_xml::events::BytesStart,
                    empty: bool
                ) -> Result<Self, rustical_xml::XmlDeError> {
                    use quick_xml::events::Event;
                    use rustical_xml::XmlDeError;

                    let mut buf = Vec::new();

                    // initialise fields
                    struct StructBuilder #type_generics #where_clause {
                        #(#builder_fields),*
                    }

                    let mut builder = StructBuilder {
                        #(#builder_field_inits),*
                    };

                    for attr in start.attributes() {
                        let attr = attr?;
                        match attr.key.as_ref() {
                            #(#attr_field_branches),*
                            _ => { #invalid_field_branch }
                        }
                    }

                    if !empty {
                        loop {
                            let event = reader.read_event_into(&mut buf)?;
                            match &event {
                                Event::End(e) if e.name() == start.name() => {
                                    break;
                                }
                                Event::Eof => return Err(XmlDeError::Eof),
                                // start of a child element
                                Event::Start(start) | Event::Empty(start) => {
                                    let empty = matches!(event, Event::Empty(_));
                                    let (ns, name) = reader.resolve_element(start.name());
                                    match (ns, name.as_ref()) {
                                        #(#named_field_branches),*
                                        #(#untagged_field_branches),*
                                        _ => { #invalid_field_branch }
                                    }
                                }
                                Event::Text(bytes_text) => {
                                    let text = bytes_text.unescape()?;
                                    #(#text_field_branches)*
                                }
                                Event::CData(cdata) => {
                                    return Err(XmlDeError::UnsupportedEvent("CDATA"));
                                }
                                Event::Comment(_) => { /* ignore */ }
                                Event::Decl(_) => {
                                    // Error: not supported
                                    return Err(XmlDeError::UnsupportedEvent("Declaration"));
                                }
                                Event::PI(_) => {
                                    // Error: not supported
                                    return Err(XmlDeError::UnsupportedEvent("Processing instruction"));
                                }
                                Event::DocType(doctype) => {
                                    // Error: start of new document
                                    return Err(XmlDeError::UnsupportedEvent("Doctype in the middle of the document"));
                                }
                                Event::End(end) => {
                                    // Error: premature end
                                    return Err(XmlDeError::Other("Unexpected closing tag for wrong element".to_owned()));
                                }
                            }
                        }
                    }

                    Ok(Self {
                        #(#builder_field_builds),*
                    })
                }
            }
        }
    }

    pub fn impl_se(&self) -> proc_macro2::TokenStream {
        let (impl_generics, type_generics, where_clause) = self.generics.split_for_impl();
        let ident = &self.ident;
        let tag_writers = self.fields.iter().map(Field::tag_writer);

        // TODO: Implement attributes
        quote! {
            impl #impl_generics ::rustical_xml::XmlSerialize for #ident #type_generics #where_clause {
                fn serialize<W: ::std::io::Write>(
                    &self,
                    tag: Option<&[u8]>,
                    writer: &mut ::quick_xml::Writer<W>
                ) -> ::std::io::Result<()> {
                    use ::quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};

                    let tag_str = tag.map(String::from_utf8_lossy);

                    if let Some(tag) = &tag_str {
                        writer.write_event(Event::Start(BytesStart::new(tag.to_owned())))?;
                    }
                    #(#tag_writers);*
                    if let Some(tag) = &tag_str {
                        writer.write_event(Event::End(BytesEnd::new(tag.to_owned())))?;
                    }
                    Ok(())
                }
            }
        }
    }
}
