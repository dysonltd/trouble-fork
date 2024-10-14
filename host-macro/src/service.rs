//! Gatt Service Builder
//!
//! This module contains the ServiceBuilder struct which is used to construct a Gatt Service from a struct definition.
//! The struct definition is used to define the characteristics of the service, and the ServiceBuilder is used to
//! generate the code required to create the service.

use crate::characteristic::{Characteristic, CharacteristicArgs};
use crate::uuid::Uuid;
use darling::FromMeta;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote, quote_spanned};
use syn::meta::ParseNestedMeta;
use syn::parse::Result;
use syn::spanned::Spanned;
use syn::LitStr;

#[derive(Debug, Default)]
pub(crate) struct ServiceArgs {
    pub uuid: Option<Uuid>,
}

impl ServiceArgs {
    pub fn parse(&mut self, meta: ParseNestedMeta) -> Result<()> {
        if meta.path.is_ident("uuid") {
            let uuid_string: LitStr = meta.value()?.parse()?;
            self.uuid = Some(Uuid::from_string(uuid_string.value().as_str())?);
            Ok(())
        } else {
            Err(meta.error("Unsupported service property, 'uuid' is the only supported property"))
        }
    }
}

pub(crate) struct ServiceBuilder {
    properties: syn::ItemStruct,
    uuid: Uuid,
    code_impl: TokenStream2,
    code_build_chars: TokenStream2,
    code_struct_init: TokenStream2,
    code_fields: TokenStream2,
    code_characteristic_handles: Vec<TokenStream2>,
}

impl ServiceBuilder {
    pub fn new(properties: syn::ItemStruct, uuid: Uuid) -> Self {
        Self {
            uuid,
            properties,
            code_struct_init: TokenStream2::new(),
            code_impl: TokenStream2::new(),
            code_fields: TokenStream2::new(),
            code_build_chars: TokenStream2::new(),
            code_characteristic_handles: Vec::new(),
        }
    }
    /// Construct the macro blueprint for the service struct.
    pub fn build(self) -> TokenStream2 {
        let properties = self.properties;
        let visibility = &properties.vis;
        let struct_name = &properties.ident;
        let code_struct_init = self.code_struct_init;
        let code_impl = self.code_impl;
        let fields = self.code_fields;
        let code_build_chars = self.code_build_chars;
        let code_characteristic_handles = &self.code_characteristic_handles;
        let uuid = self.uuid;

        quote! {
            #visibility struct #struct_name<M: embassy_sync::blocking_mutex::raw::RawMutex> {
                handle: AttributeHandle,
                #fields
            }

            #[allow(unused)]
            impl<M: embassy_sync::blocking_mutex::raw::RawMutex> #struct_name<M> {
                #visibility fn new<const MAX_ATTRIBUTES: usize>(table: &mut AttributeTable<'_, M, MAX_ATTRIBUTES>) -> Self
                {
                    let mut service = table.add_service(Service::new(#uuid));
                    #code_build_chars

                    Self {
                        handle: service.build(),
                        #code_struct_init
                    }
                }
                #visibility fn has_characteristic_with_handle(&self, handle: u16) -> bool {
                    let handles = [#(#code_characteristic_handles)|*];
                    handles.contains(&handle)
                }
                #code_impl
            }
        }
    }

    /// Construct instructions for adding a characteristic to the service, with static storage.
    fn construct_characteristic_static(
        &mut self,
        name: &str,
        span: Span,
        ty: &syn::Type,
        properties: &Vec<TokenStream2>,
        uuid: Option<Uuid>,
    ) {
        let name_screaming = format_ident!(
            "{}",
            inflector::cases::screamingsnakecase::to_screaming_snake_case(name)
        );
        let char_name = format_ident!("{}", name);
        self.code_build_chars.extend(quote_spanned! {span=>
            let #char_name = {
                static #name_screaming: static_cell::StaticCell<[u8; size_of::<#ty>()]> = static_cell::StaticCell::new();
                let store = #name_screaming.init([0; size_of::<#ty>()]);
                let builder = service.add_characteristic(#uuid, &[#(#properties),*], store);

                // TODO: Descriptors

                builder.build()
            };
        });

        self.code_struct_init.extend(quote_spanned!(span=>
            #char_name,
        ));
    }

    /// Consume the lists of fields and fields marked as characteristics and prepare the code to add them to the service
    /// by generating the macro blueprints for any methods, fields, and static storage required.
    pub fn process_characteristics_and_fields(
        mut self,
        mut fields: Vec<syn::Field>,
        characteristics: Vec<Characteristic>,
    ) -> Self {
        // Processing specific to non-characteristic fields
        for field in &fields {
            let ident = field.ident.as_ref().expect("All fields should have names");
            let ty = &field.ty;
            self.code_struct_init.extend(quote_spanned! {field.span() =>
                #ident: #ty::default(),
            })
        }

        // Process characteristic fields
        for ch in characteristics {
            let handle_ident = format_ident!("{}", ch.name);
            let store_ident = format_ident!("{}_store", ch.name);
            let read_callback_ident = format_ident!("{}_on_read", ch.name);
            let read_callback = if let Some(callback) = &ch.args.on_read {
                quote!(Some(#callback))
            } else {
                quote!(None)
            };
            let write_callback_ident = format_ident!("{}_on_write", ch.name);
            let write_callback = if let Some(callback) = &ch.args.on_write {
                quote!(Some(#callback))
            } else {
                quote!(None)
            };
            let uuid = ch.args.uuid;

            // TODO add methods to characteristic
            let _get_fn = format_ident!("{}_get", ch.name);
            let _set_fn = format_ident!("{}_set", ch.name);
            let _notify_fn = format_ident!("{}_notify", ch.name);
            let _indicate_fn = format_ident!("{}_indicate", ch.name);
            let _fn_vis = &ch.vis;

            let _notify = ch.args.notify;
            let _indicate = ch.args.indicate;

            let ty = &ch.ty;

            let properties = set_access_properties(&ch.args);

            // add fields for each characteristic value handle
            fields.push(syn::Field {
                ident: Some(handle_ident.clone()),
                ty: syn::Type::Verbatim(quote!(Characteristic)),
                attrs: Vec::new(),
                colon_token: Default::default(),
                vis: syn::Visibility::Inherited,
                mutability: syn::FieldMutability::None,
            });

            if !ch.args.app_managed {
                // add field for characteristic data storage
                fields.push(syn::Field {
                    ident: Some(store_ident.clone()),
                    ty: syn::Type::Verbatim(quote!(embassy_sync::blocking_mutex::Mutex<M, core::cell::RefCell<#ty>>)),
                    attrs: Vec::new(),
                    colon_token: Default::default(),
                    vis: syn::Visibility::Inherited,
                    mutability: syn::FieldMutability::None,
                });

                self.code_struct_init.extend(quote_spanned! {ch.span=>
                    #store_ident: embassy_sync::blocking_mutex::Mutex::new(core::cell::RefCell::new(<#ty>::default())),
                });
            }

            self.code_characteristic_handles
                .push(quote! {self.#handle_ident.handle()});

            fields.push(syn::Field {
                ident: Some(read_callback_ident.clone()),
                ty: syn::Type::Verbatim(quote!(Option<fn(::trouble_host::connection::Connection) -> &[u8]>)),
                attrs: Vec::new(),
                vis: syn::Visibility::Inherited,
                mutability: syn::FieldMutability::None,
                colon_token: Default::default(),
            });

            fields.push(syn::Field {
                attrs: Vec::new(),
                vis: syn::Visibility::Inherited,
                mutability: syn::FieldMutability::None,
                ident: Some(write_callback_ident.clone()),
                colon_token: Default::default(),
                ty: syn::Type::Verbatim(quote!(Option<fn(::trouble_host::connection::Connection, &[u8])>)),
            });

            self.code_struct_init.extend(quote_spanned! {ch.span=>
                #read_callback_ident: #read_callback,
                #write_callback_ident: #write_callback,
            });

            self.construct_characteristic_static(&ch.name, ch.span, ty, &properties, uuid);
        }

        // Processing common to all fields
        for field in fields {
            let ident = field.ident.clone();
            let ty = field.ty.clone();
            self.code_fields.extend(quote_spanned! {field.span()=>
                #ident: #ty,
            })
        }
        self
    }
}

fn parse_property_into_list(property: bool, variant: TokenStream2, properties: &mut Vec<TokenStream2>) {
    if property {
        properties.push(variant);
    }
}

/// Parse the properties of a characteristic and return a list of properties
fn set_access_properties(args: &CharacteristicArgs) -> Vec<TokenStream2> {
    let mut properties = Vec::new();
    parse_property_into_list(args.read, quote! {CharacteristicProp::Read}, &mut properties);
    parse_property_into_list(args.write, quote! {CharacteristicProp::Write}, &mut properties);
    parse_property_into_list(
        args.write_without_response,
        quote! {CharacteristicProp::WriteWithoutResponse},
        &mut properties,
    );
    parse_property_into_list(args.notify, quote! {CharacteristicProp::Notify}, &mut properties);
    parse_property_into_list(args.indicate, quote! {CharacteristicProp::Indicate}, &mut properties);
    properties
}
